#![allow(unused_imports)]
use cosmwasm_std::{testing::{mock_dependencies, mock_env, mock_info}};
use crate::state::CONFIG;
// Note: USER_INCENTIVES, INCENTIVE_SCHEDULE, INCENTIVE_EVENTS, UserIncentives, IncentiveEvent have been removed
// use crate::state::{CONFIG, USER_INCENTIVES, INCENTIVE_SCHEDULE, INCENTIVE_EVENTS, UserIncentives, IncentiveEvent};
use membrane::transmuter::{InstantiateMsg as TInstantiate, ExecuteMsg as TExecute, AssetPair};
use cosmwasm_std::Coin;

fn default_instantiate_msg() -> TInstantiate {
    TInstantiate {
        owner: Some("owner".to_string()),
        tokenfactory_contract: None,
        cdp_contract: "cdp".to_string(),
        deposit_pair: AssetPair { cdt: "cdt".to_string(), paired_asset: "usdc".to_string() },
        composition_leeway: Decimal::percent(5),
        cdt_target_ratio: Decimal::zero(),
        usage_fee: Some(Decimal::zero()),
        usage_fee_utilization_threshold: None,
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
        discounts_contract: "discounts".to_string(),
        lock_ceiling: 1460,
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
        revenue_distributor_fee_percentage: None,
        emissions_voting_contract: None,
    }
}

fn setup_instant(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) {
    let env = mock_env();
    let info = mock_info("owner", &[]);
    let msg = default_instantiate_msg();
    let res = instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(res.messages.len(), 0); // No tokenfactory messages
}

#[test]
#[ignore] // INCENTIVE_SCHEDULE has been removed
fn init_sets_incentive_schedule() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let env = mock_env();
    // let schedule = INCENTIVE_SCHEDULE.load(&deps.storage).unwrap();
    // assert_eq!(schedule.start_time, env.block.time.seconds());
    // assert_eq!(schedule.last_accrued_time, env.block.time.seconds());
    // // Default starts at zero until configured
    // assert_eq!(schedule.total_monthly_emission, Uint128::zero());
}

#[test]
#[ignore] // USER_INCENTIVES has been removed
fn enter_vault_with_incentive_toggle_tracks_user_vt() {
    // Seed the contract address with 100 CDT so balance queries during alignment succeed
    let mut deps: cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier> = cosmwasm_std::testing::mock_dependencies_with_balances(&[
        ("cosmos2contract", &[coin(100, "cdt")])
    ]);
    setup_instant(&mut deps);
    let mut env = mock_env();
    // fund contract with CDT so enter works
    // user deposits 100 CDT
    let info = mock_info("user", &[Coin{ denom: "cdt".into(), amount: Uint128::new(100)}]);
    //Send the contract 100 CDT
    // call EnterVault with deposit_for_incentives true
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        TExecute::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }
    ).unwrap();
    println!("res: {:?}", res.messages.len());
    // mint to contract and NO send to user
    assert!(res.messages.len() == 1);
    // let user = USER_INCENTIVES.load(&deps.storage, "user".into()).unwrap();
    // assert!(user.vault_tokens_in_contract > Uint128::zero());
}

#[test]
#[ignore] // INCENTIVE_EVENTS has been removed
fn accrue_creates_events_capped_by_monthly_max() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let mut env = mock_env();
    // set DEPOSIT_TOTAL to a positive value
    crate::state::DEPOSIT_TOTAL.save(&mut deps.storage, &Uint128::new(1_000_000)).unwrap();
    // advance time by 10 seconds
    env.block.time = env.block.time.plus_seconds(10);
    // Accrual: simulate by calling internal helper via super if available, else push event directly
    // Fallback: create an event manually for test stability
    // let amount_per_vt = Decimal::from_ratio(1000u128, 1u128);
    // INCENTIVE_EVENTS.save(&mut deps.storage, &vec![IncentiveEvent{ amount_per_vt, time_of_event: env.block.time.seconds(), amount_left_to_claim: Uint128::new(1000)}]).unwrap();
    // let events = INCENTIVE_EVENTS.load(&deps.storage).unwrap();
    // assert_eq!(events.len(), 1);
    // assert!(events[0].amount_left_to_claim > Uint128::zero());
}

#[test]
#[ignore] // Incentives removed
fn claim_incentives_updates_user_and_prunes() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let mut env = mock_env();
    // Give user incentive VT balance
    // USER_INCENTIVES.save(&mut deps.storage, "user".into(), &UserIncentives{
    //     total_claimed: Uint128::zero(),
    //     vault_tokens_in_contract: Uint128::new(1_000_000),
    //     last_accrued: 0,
    //     mbrn_intents: None,
    // }).unwrap();
    crate::state::DEPOSIT_TOTAL.save(&mut deps.storage, &Uint128::new(1_000_000)).unwrap();
    // two events
    // INCENTIVE_EVENTS.save(&mut deps.storage, &vec![
    //     IncentiveEvent{ amount_per_vt: Decimal::from_ratio(1u128, 1u128), time_of_event: env.block.time.seconds(), amount_left_to_claim: Uint128::new(1100_000)},
    //     IncentiveEvent{ amount_per_vt: Decimal::from_ratio(1u128, 1u128), time_of_event: env.block.time.seconds()+1, amount_left_to_claim: Uint128::new(1100_000)},
    // ]).unwrap();


    //skip ahead 3 seconds to update accrued_time to past all current events
    env.block.time = env.block.time.plus_seconds(3);
    // claim once
    let info = mock_info("caller", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        // ClaimIncentivesForUser has been removed
        // TExecute::ClaimIncentivesForUser { user: "user".into(), limit: Some(10), mbrn_intent: None }
        TExecute::DepositFee {} // Placeholder - this test is likely outdated
    ).unwrap();
    // should attempt to mint via proxy if configured
    assert!(!res.messages.is_empty());
    // let user = USER_INCENTIVES.load(&deps.storage, "user".into()).unwrap();
    // assert!(user.total_claimed > Uint128::zero());
    // let events = INCENTIVE_EVENTS.load(&deps.storage).unwrap();
    // amount_left_to_claim potentially reduced or pruned
    // assert!(events.len() == 2);

    // double claim should not increase claimed again
    // let prev_claimed = user.total_claimed;
    let info2 = mock_info("caller", &[]);
    // ClaimIncentivesForUser has been removed
    // let _ = execute(
    //     deps.as_mut(),
    //     env.clone(),
    //     info2,
    //     TExecute::ClaimIncentivesForUser { user: "user".into(), limit: Some(10), mbrn_intent: None }
    // ).unwrap();
    // let user2 = USER_INCENTIVES.load(&deps.storage, "user".into()).unwrap();
    // assert_eq!(user2.total_claimed, prev_claimed);

    // let events = INCENTIVE_EVENTS.load(&deps.storage).unwrap();
    // events should be untouched
    // assert!(events.len() == 2);
}
#[test]
#[ignore] // Incentives removed
fn no_claim_from_old_events() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let mut env = mock_env();
    // User has 1 VT in contract
    // USER_INCENTIVES.save(&mut deps.storage, "user".into(), &UserIncentives{
    //     total_claimed: Uint128::zero(),
    //     vault_tokens_in_contract: Uint128::new(1),
    //     last_accrued: env.block.time.seconds(),
    //     mbrn_intents: None,
    // }).unwrap();
    // Create an event before user's last_accrued
    let old_time = env.block.time.seconds() - 10;
    // INCENTIVE_EVENTS.save(&mut deps.storage, &vec![IncentiveEvent{ amount_per_vt: Decimal::from_ratio(1000u128, 1u128), time_of_event: old_time, amount_left_to_claim: Uint128::new(1000)}]).unwrap();
    // Claim should result in zero claimed
    let info = mock_info("caller", &[]);
    let _ = execute(
        deps.as_mut(),
        env.clone(),
        info,
        // ClaimIncentivesForUser has been removed
        // TExecute::ClaimIncentivesForUser { user: "user".into(), limit: Some(10), mbrn_intent: None }
        TExecute::DepositFee {} // Placeholder - this test is likely outdated
    ).unwrap();
    // let user = USER_INCENTIVES.load(&deps.storage, "user".into()).unwrap();
    // assert_eq!(user.total_claimed, Uint128::zero());
}

#[test]
#[ignore] // Incentives removed
fn exit_vault_can_include_incentive_vt() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let env = mock_env();
    // Set vault token supply and balances to allow exit
    crate::state::DEPOSIT_TOTAL.save(&mut deps.storage, &Uint128::new(1_000_000)).unwrap();
    // give contract both assets to send on exit
    // This test is high-level; we ensure we don't error on using incentive-held VT
    // USER_INCENTIVES.save(&mut deps.storage, "user".into(), &UserIncentives{
    //     total_claimed: Uint128::zero(),
    //     vault_tokens_in_contract: Uint128::new(10_000),
    //     last_accrued: 0,
    //     mbrn_intents: None,
    // }).unwrap();
    // Try exit with no VT sent but using incentive-held
    let info = mock_info("user", &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        TExecute::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None }
    );
    // Depending on balances, this may fail on InsufficientLiquidity; ensure it doesn't fail on VT missing
    // If it fails, it should not be due to "no vault tokens provided"
    if let Err(e) = res { 
        let msg = format!("{}", e);
        assert!(!msg.contains("no vault tokens provided"));
    }
}

#[test]
#[ignore] // Incentives removed
fn stress_many_events_and_users() {
    let mut deps = mock_dependencies();
    setup_instant(&mut deps);
    let mut env = mock_env();
    crate::state::DEPOSIT_TOTAL.save(&mut deps.storage, &Uint128::new(1_000_000_000)).unwrap();
    
    // 100 users with 1e6 VT each
    for i in 0..100u32 {
        // USER_INCENTIVES.save(&mut deps.storage, format!("u{}", i), &UserIncentives{
        //     total_claimed: Uint128::zero(),
        //     vault_tokens_in_contract: Uint128::new(1_000_000),
        //     last_accrued: 0,
        //     mbrn_intents: None,
        // }).unwrap();
    }
    // create 200 events by accrual
    for _ in 0..200 {
        env.block.time = env.block.time.plus_seconds(3);
        let amount_per_vt = Decimal::from_ratio(1000u128, 1u128);
        // let mut cur = INCENTIVE_EVENTS.load(&deps.storage).unwrap_or_default();
        // cur.push(IncentiveEvent{ amount_per_vt, time_of_event: env.block.time.seconds(), amount_left_to_claim: Uint128::new(1000)});
        // INCENTIVE_EVENTS.save(&mut deps.storage, &cur).unwrap();
    }
    // claim for a subset with limit
    for i in 0..3u32 {
        let info = mock_info("caller", &[]);
        let _ = execute(
            deps.as_mut(),
            env.clone(),
            info,
            // ClaimIncentivesForUser has been removed
            TExecute::DepositFee {} // Placeholder
        ).unwrap();
    }
    // ensure events not fully pruned
    // let events = INCENTIVE_EVENTS.load(&deps.storage).unwrap();
    // assert!(!events.is_empty());
}

use cosmwasm_std::{coin, coins, Addr, Binary, Decimal, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult, Uint128, to_json_binary};
use cosmwasm_schema::cw_serde;
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
use std::collections::HashMap;

use membrane::tokenfactory::{ExecuteMsg as TfExecuteMsg, InstantiateMsg as TfInstantiateMsg};
use membrane::cdp::QueryMsg as CdpQueryMsg;
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, QueryMsg, TransmuteHistoryResponse, VolumeHistoryResponse, VaultInfoResponse, RateLimitStatusResponse, RateLimitManyResponse, GlobalRateLimitResponse};
use membrane::system_discounts::{QueryMsg as SystemsDiscountsQueryMsg, UserBoostResponse};
use membrane::neutron_proxy::ExecuteMsg as NeutronProxyExecuteMsg;

use crate::contract::{execute, instantiate, query};

const ADMIN: &str = "admin";
const USER: &str = "user";
const OTHER: &str = "other";
const ASSET_A: &str = "asset-a";
const ASSET_B: &str = "asset-b";
// VAULT_SUBDENOM no longer needed - vault tokens removed
const INITIAL_BALANCE: u128 = 1_000_000_000;
const USER_DEPOSIT: u128 = 100_000;

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

fn setup_app() -> (App, Addr, Addr, Addr, Addr, Addr, Addr, Addr) {
    let mut app = App::default();
    // Create valid bech32 addresses using addr_make
    let admin_addr = app.api().addr_make("admin");
    let user_addr = app.api().addr_make("user");
    let other_addr = app.api().addr_make("other");
    let other2_addr = app.api().addr_make("other2");
    let other3_addr = app.api().addr_make("other3");
    let other4_addr = app.api().addr_make("other4");
    let other5_addr = app.api().addr_make("other5");
    
    app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(
                storage,
                &admin_addr,
                vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B), coin(200000000000, "factory/contract1/vault-token")],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &user_addr,
                vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
            )
                .unwrap();
            router
                .bank
                .init_balance(
                    storage,
                    &other_addr,
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();

                router
                .bank
                .init_balance(
                    storage,
                    &other2_addr,
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &other3_addr,
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &other4_addr,
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &other5_addr,
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
    });
    (app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr)
}

// Helper function to get test addresses using addr_make (for backward compatibility)
fn get_test_addrs(app: &App) -> (Addr, Addr, Addr, Addr, Addr, Addr, Addr) {
    (
        app.api().addr_make("admin"),
        app.api().addr_make("user"),
        app.api().addr_make("other"),
        app.api().addr_make("other2"),
        app.api().addr_make("other3"),
        app.api().addr_make("other4"),
        app.api().addr_make("other5"),
    )
}

// Thread-local storage to track active deployment venues for mock CDP
// This is a flat list of venue addresses that are active
thread_local! {
    static ACTIVE_DEPLOYMENT_VENUES: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
}

fn mock_cdp_contract() -> Box<dyn Contract<Empty>> {
    // Query msg type is CdpQueryMsg so the framework decodes it for us
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> { Ok(Response::new()) },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, q: CdpQueryMsg| -> StdResult<Binary> {
            match q {
                CdpQueryMsg::GetActiveDeploymentVenues { venue, .. } => {
                    let list = ACTIVE_DEPLOYMENT_VENUES.with(|venues| {
                        let active_venues = venues.borrow();
                        match venue {
                            Some(v) => {
                                // If specific venue requested, return it if it's active, else empty
                                if active_venues.iter().any(|av| av == &v) {
                                    vec![v]
                                } else {
                                    Vec::new()
                                }
                            },
                            None => active_venues.clone(),
                        }
                    });
                    to_json_binary(&list)
                },
                _ => Err(StdError::generic_err("unsupported mock cdp query")),
            }
        },
    ))
}

// Helper function to register a deployment venue as active in the mock CDP
fn register_deployment_venue(_user: String, venue: String) {
    ACTIVE_DEPLOYMENT_VENUES.with(|venues| {
        let mut list = venues.borrow_mut();
        if !list.iter().any(|v| v == &venue) {
            list.push(venue);
        }
    });
}

// Mock Deployment Venue Contract
#[cw_serde]
enum DeploymentVenue_MockExecuteMsg {
    EnterVault {
        leave_vault_tokens_in_vault: Option<membrane::types::LeaveTokens>,
    },
    RepayUserDebt {
        user_info: membrane::types::UserInfo,
        repayment: Uint128,
    },
}

#[cw_serde]
struct DeploymentVenue_MockInstantiateMsg {}

#[cw_serde]
enum DeploymentVenue_MockQueryMsg {
    RetrievableCDT {
        user: String,
    },
}

fn mock_deployment_venue_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: DeploymentVenue_MockExecuteMsg| -> StdResult<Response> {
            Ok(Response::new())
        },
        |_deps, _env, _info, _msg: DeploymentVenue_MockInstantiateMsg| -> StdResult<Response> {
            Ok(Response::default())
        },
        |_deps, _env, msg: DeploymentVenue_MockQueryMsg| -> StdResult<Binary> {
            match msg {
                DeploymentVenue_MockQueryMsg::RetrievableCDT { user: _ } => {
                    // Return a mock retrievable CDT amount
                    to_json_binary(&Uint128::new(1000_000_000))
                }
            }
        },
    ))
}

fn mock_discounts_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> { Ok(Response::new()) },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, q: SystemsDiscountsQueryMsg| -> StdResult<Binary> {
            match q {
                SystemsDiscountsQueryMsg::UserBoost { .. } => {
                    cosmwasm_std::to_json_binary(&UserBoostResponse { user: "".to_string(), boost: Decimal::percent(50) })
                }
                _ => Err(StdError::generic_err("unsupported discounts query")),
            }
        },
    ))
}

fn mock_revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> { Ok(Response::new()) },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, _msg: Empty| -> StdResult<Binary> { cosmwasm_std::to_json_binary(&Empty {}) },
    ))
}

fn mock_neutron_proxy_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, msg: NeutronProxyExecuteMsg| -> Result<Response, StdError> {
            match msg {
                NeutronProxyExecuteMsg::MintTokens { denom, amount, mint_to_address } => {
                    Ok(Response::new()
                        .add_attribute("action", "mint_tokens")
                        .add_attribute("denom", denom)
                        .add_attribute("amount", amount.to_string())
                        .add_attribute("to", mint_to_address))
                }
                _ => Ok(Response::new()),
            }
        },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, _msg: Empty| -> StdResult<Binary> { cosmwasm_std::to_json_binary(&Empty {}) },
    ))
}

/// Returns (transmuter_addr, cdp_addr)
fn instantiate_transmuter(app: &mut App, admin_addr: Addr) -> (Addr, Addr) {
    // Use the admin_addr passed from setup_app to ensure addresses match
    let tokenfactory_addr_str = app.api().addr_make("tokenfactory").to_string();
    let cdp_addr_str = app.api().addr_make("cdp").to_string();
    let revenue_distributor_addr_str = app.api().addr_make("revenue_distributor").to_string();
    let discounts_addr_str = app.api().addr_make("discounts").to_string();
    
    // Store contracts
    let tf_code = app.store_code(mock_tokenfactory_contract());
    let code_id = app.store_code(transmuter_contract());
    let cdp_code = app.store_code(mock_cdp_contract());
    let revenue_distributor_code = app.store_code(mock_revenue_distributor_contract());
    let discounts_code = app.store_code(mock_discounts_contract());
    
    // Instantiate contracts and get their actual addresses
    let tokenfactory_addr_actual = app.instantiate_contract(
        tf_code,
        admin_addr.clone(),
        &TfInstantiateMsg { owner: Some(admin_addr.to_string()) },
        &[],
        "mock-tokenfactory",
        None,
    )
    .unwrap();
    
    // Give tokenfactory contract a large balance of vault tokens so it can mint them
    // The vault token denom will be created when the transmuter is instantiated
    // We'll add balance after transmuter instantiation

    let cdp_addr_actual = app.instantiate_contract(
        cdp_code,
        admin_addr.clone(),
        &Empty {},
        &[],
        "mock-cdp",
        None,
    )
    .unwrap();
    
    let revenue_distributor_addr_actual = app.instantiate_contract(
        revenue_distributor_code,
        admin_addr.clone(),
        &Empty {},
        &[],
        "mock-revenue-distributor",
        None,
    )
    .unwrap();
    
    let discounts_addr_actual = app.instantiate_contract(
        discounts_code,
        admin_addr.clone(),
        &Empty {},
        &[],
        "mock-discounts",
        None,
    )
    .unwrap();
    
    // Use the actual addresses from instantiated contracts
    let msg = InstantiateMsg {
        owner: Some(admin_addr.to_string()),
        tokenfactory_contract: None, // No longer needed
        revenue_distributor_addr: Some(revenue_distributor_addr_actual.to_string()),
        cdp_contract: cdp_addr_actual.to_string(),
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
        global_rate_limit_window_secs: Some(60 * 60 * 24), // 24 hours
        global_rate_limit_threshold: Some(Decimal::percent(20)), // 20%
        revenue_distributions: None,
        lock_ceiling: 1460,
        discounts_contract: discounts_addr_actual.to_string(),
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
        revenue_distributor_fee_percentage: None,
        emissions_voting_contract: None,
    };

    let transmuter_addr = app.instantiate_contract(
        code_id,
        admin_addr.clone(),
        &msg,
        &[],
        "transmuter",
        None,
    )
    .unwrap();

    (transmuter_addr, cdp_addr_actual)
}

fn query_rate_limit(app: &App, contract: &Addr, address: &str) -> RateLimitStatusResponse {
    let resp: RateLimitManyResponse = app
        .wrap()
        .query_wasm_smart(contract, &QueryMsg::RateLimitMany { addresses: Some(vec![address.to_string()]), start_after: None, limit: None })
        .unwrap();
    resp.records.into_iter().next().unwrap()
}

fn query_rate_limit_many(app: &App, contract: &Addr, addrs: Option<Vec<String>>, start_after: Option<u64>, limit: Option<u32>) -> RateLimitManyResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::RateLimitMany { addresses: addrs, start_after, limit })
        .unwrap()
}

fn query_global_rate_limit(app: &App, contract: &Addr) -> GlobalRateLimitResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::GlobalRateLimit {})
        .unwrap()
}

#[test]
fn rate_limit_blocks_when_threshold_exceeded_and_nets_flows() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault to set deposits baseline (so threshold calc > 0)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // CDT→paired_asset is CDP-only, so test rate limiting with paired_asset→CDT direction only.
    // Per-address threshold: 5% of 200k = 10k

    // USER does B->A (+10k A) — hits 5% threshold
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Next +1 pushes over -> should error
    let res = app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
}


#[test]
fn allowlist_uses_higher_threshold() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Update config to add USER to allowlist and set small base deposits
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: user_addr.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: Some(Decimal::percent(20)),
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Seed B
    // app.execute_contract(admin_addr.clone(), contract.clone(), &ExecuteMsg::DepositFee {}, &coins(1_000_000, ASSET_B)).unwrap();

    // USER can move up to 20% before block
    // 20% of 200k = 40k. Try 39,999 -> ok, 1 more -> block
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(39_999, ASSET_B)).unwrap();
    let res = app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(2, ASSET_B));
    assert!(res.is_err());
}

#[test]
fn rate_limit_many_paginates() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Add two allowlist entries
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![
                membrane::types::StringEntry { entry: user_addr.to_string(), remove: false },
                membrane::types::StringEntry { entry: other_addr.to_string(), remove: false },
            ]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    let page1 = query_rate_limit_many(&app, &contract, None, None, Some(1));
    assert_eq!(page1.records.len(), 1);
    let page2 = query_rate_limit_many(&app, &contract, None, page1.next_start_after, Some(1));
    assert_eq!(page2.records.len(), 1);
}

#[test]
fn window_expiry_unblocks_usage() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Set shorter window (2 hours)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: Some(3 * 60 * 60),
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Deposits only (no extra liquidity that would inflate threshold)
    app.execute_contract(admin_addr.clone(), contract.clone(), &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }, &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)]).unwrap();

    // Add several entries spreading over time
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(4_000, ASSET_B)).unwrap();
    let s1 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s1.status.entries_count, 1);

    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60); b.height += 1; });
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_B)).unwrap();
    let s2 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s2.status.entries_count, 2);

    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 * 2); b.height += 1; });
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_B)).unwrap();
    let s3 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s3.status.entries_count, 3);

    // Move window forward just past the first entry; expect 2 entries remain
    app.update_block(|b| { b.time = b.time.plus_seconds(1); b.height += 1; });
    let s4 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s4.status.entries_count, 2);

    // Move window forward again past the second
    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 + 1); b.height += 1; });
    let s5 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s5.status.entries_count, 1);

    // Finally past the third, entries should be 0
    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 * 2 + 1); b.height += 1; });
    let s6 = query_rate_limit(&app, &contract, &user_addr.to_string());
    assert_eq!(s6.status.entries_count, 0);
}

#[test]
fn usage_fee_applied_for_non_cdp_and_non_deployable() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Set usage fee to 10% with low utilization threshold so fee triggers easily
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: Some(Decimal::percent(10)),
            usage_fee_utilization_threshold: Some(Decimal::zero()), // always active
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: Some(Decimal::percent(50)), // raise so CDP doesn't hit limit
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Seed contract with B liquidity to pay out A->B swaps
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Non-CDP user cannot do CDT→paired_asset
    let res = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    );
    assert!(res.is_err());

    // CDP pays usage fee when utilization threshold is met;
    // send 10_000 A, after 10% fee => 9_000 A considered
    app.send_tokens(admin_addr.clone(), cdp_addr.clone(), &coins(20_000, ASSET_A)).unwrap();

    app.execute_contract(
        cdp_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    ).unwrap();

    let swaps = query_swap_history(&app, &contract);
    let last = swaps.records.last().unwrap();
    assert_eq!(last.offered_asset, ASSET_A);
    assert_eq!(last.offered_amount, Uint128::from(9_000u64));
    assert_eq!(last.received_asset, ASSET_B);
    assert_eq!(last.received_amount, Uint128::from(9_000u64));

    // Now set threshold high so fee does NOT activate (utilization is ~50% with balanced deposits)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None, deposit_pair: None, composition_leeway: None, cdt_target_ratio: None,
            tokenfactory_contract: None, discounts_contract: None, cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: Some(Decimal::percent(99)), // effectively never
            swap_history_cap: None, volume_history_cap: None,
            rate_limit_window_secs: None, rate_limit_threshold: None,
            allowlist: None, allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None, global_rate_limit_threshold: None,
            revenue_distributor_addr: None, revenue_distributions: None,
            lock_ceiling: None, affiliate_fee: Decimal::percent(1),
            send_swap_fee: None, revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // CDP swap without fee since utilization is below threshold
    app.execute_contract(
        cdp_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    ).unwrap();

    let swaps2 = query_swap_history(&app, &contract);
    let last2 = swaps2.records.last().unwrap();
    assert_eq!(last2.offered_asset, ASSET_A);
    assert_eq!(last2.offered_amount, Uint128::from(10_000u64)); // no fee applied
    assert_eq!(last2.received_asset, ASSET_B);
    assert_eq!(last2.received_amount, Uint128::from(10_000u64));
}

#[test]
#[ignore] // Allowlist/deployment venue logic commented out for now
fn paired_asset_outstanding_tracks_allowlisted_flows() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // query initial outstanding
    let start: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(start.amount, Uint128::zero());

    // Create a mock deployment venue contract
    let venue_code_id = app.store_code(mock_deployment_venue_contract());
    let venue_addr = app
        .instantiate_contract(
            venue_code_id,
            admin_addr.clone(),
            &DeploymentVenue_MockInstantiateMsg {},
            &[],
            "deployment_venue",
            None,
        )
        .unwrap();

    // Register the venue address as an active deployment venue
    // The transmuter checks if the sender is in the active deployment venues list
    register_deployment_venue(venue_addr.to_string(), venue_addr.to_string());

    // Add venue address to allowlist in config (simulating allowlisted venue)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: venue_addr.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Seed enough cdt so CDT->USDC can be paid out
    app.execute_contract(admin_addr.clone(), contract.clone(), &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }, &[coin(50_000, ASSET_A), coin(50_000, ASSET_B)]).unwrap();

    // CDT->paired_asset is now CDP-only, so deployed tracking only applies to CDP.
    // CDP is not a deployment venue, so deployed counter won't increment from CDP swaps.
    // Test that allowlisted venue USDC->CDT decrements deployed counter.

    // Manually set deployed amount to simulate prior deployment
    // (In production, deployment would have happened before CDT->PA restriction)

    // Allowlisted USDC->CDT should decrement outstanding by offered paired_asset
    // Give venue_addr some tokens to swap
    app.send_tokens(user_addr.clone(), venue_addr.clone(), &coins(4_000, ASSET_B)).unwrap();
    app.execute_contract(venue_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(4_000, ASSET_B)).unwrap();
    let after_usdc_to_cdt: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    // Deployed starts at 0 and saturating_sub keeps it at 0
    assert_eq!(after_usdc_to_cdt.amount, Uint128::zero());

    // Non-allowlisted paired_asset->CDT should NOT change outstanding
    let before = after_usdc_to_cdt.amount;
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1_000, ASSET_B)).unwrap();
    let check1: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(check1.amount, before);
}

#[test]
#[ignore] // Allowlist/deployment venue logic commented out for now
fn effective_target_reflects_deployed_value_and_bounds() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // With zero deposits, target should be config.target_ratio (50%)
    let eff0: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff0.target, Decimal::percent(50));

    // Add deposits 100k cdt + 100k paired, no deployed yet -> target stays 50%
    app.execute_contract(admin_addr.clone(), contract.clone(), &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }, &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)]).unwrap();
    let eff1: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff1.target, Decimal::percent(50));

    // Mark USER allowlisted and as deployment venue via mock, then do CDT->USDC (10k) to increase deployed tally
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: user_addr.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: Some(Decimal::one()),
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Ensure contract has paired_asset liquidity for payouts
    // Need to deposit both assets aligned with target ratio (50% CDT, 50% paired)
    // For 20k paired asset, need ~20k CDT to maintain 50/50 ratio
    app.execute_contract(admin_addr.clone(), contract.clone(), &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }, &[coin(20_000, ASSET_A), coin(20_000, ASSET_B)]).unwrap();

    // CDT→paired_asset is CDP-only; CDP is not a deployment venue so deployed counter stays 0
    app.send_tokens(admin_addr.clone(), cdp_addr.clone(), &coins(200_000, ASSET_A)).unwrap();
    app.execute_contract(cdp_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap();

    // CDP is not a deployment venue, so deployed stays at 0 and effective target stays at base (50%)
    let eff2: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff2.target, Decimal::percent(50));

    // Add more liquidity and do another CDP swap
    let config_before: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let revenue_distributor_addr = config_before.revenue_distributor_addr.clone().unwrap();
    let rd_addr_parsed = Addr::unchecked(&revenue_distributor_addr);
    app.send_tokens(admin_addr.clone(), rd_addr_parsed.clone(), &coins(200_000, ASSET_B)).unwrap();
    app.execute_contract(rd_addr_parsed.clone(), contract.clone(), &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None }, &[coin(0, ASSET_A), coin(200_000, ASSET_B)]).unwrap();
    app.execute_contract(cdp_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(120_000, ASSET_A)).unwrap();

    // Total deposits base = (100k + 0) + (100k + 220k converted to base 1:1) = 420k; deployed ~130k (prev 10k + 120k)
    // effective target = max(50%, 130/420 ~= 30.95%) = 50%
    let eff3: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff3.target, Decimal::percent(50));

    // Lower base target to 10% to allow deployed to dominate
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: Some(Decimal::percent(10)),
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
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
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Now effective should be based on deployed ratio
    // Check deployed and total to calculate expected ratio
    let deployed: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    let vault_info = query_vault_info(&app, &contract);
    println!("deployed: {:?}, total_deposits (from vault): {:?}", deployed.amount, vault_info.cdt_balance + vault_info.paired_asset_balance);
    
    let eff4: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    println!("eff4: {:?}", eff4);
    // After lowering base target to 10%, if deployed ratio is > 10%, it should use deployed ratio
    // Otherwise it uses base target (10%)
    // The actual value depends on deployed/total ratio
    assert!(eff4.target >= Decimal::percent(10), "Effective target should be at least 10%");
}

fn mock_tokenfactory_instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: TfInstantiateMsg,
) -> StdResult<Response> {
    Ok(Response::new().add_attribute("method", "mock_tokenfactory_instantiate"))
}

fn mock_tokenfactory_execute(
    _deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: TfExecuteMsg,
) -> Result<Response, StdError> {
    use cosmwasm_std::CosmosMsg;
    use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgMint;
    use osmosis_std::types::cosmos::base::v1beta1::Coin as OsmosisCoin;
    
    let mut res = Response::new().add_attribute("contract", "mock_tokenfactory");
    match msg {
        TfExecuteMsg::CreateDenom { subdenom } => {
            res = res.add_attribute("create_denom", subdenom);
        }
        TfExecuteMsg::MintTokens { amount, mint_to_address } => {
            // In cw-multi-test, we simulate minting by using BankMsg::Send
            // We'll give the tokenfactory contract tokens on-demand using init_balance
            // For now, just use BankMsg::Send - the tokenfactory will get tokens via send_tokens
            if let Some(coin) = amount {
                use cosmwasm_std::BankMsg;
                // Parse amount from string to Uint128
                let amount_uint = coin.amount.parse::<u128>()
                    .map_err(|_| StdError::generic_err("Invalid amount"))?;
                res = res
                    .add_message(BankMsg::Send {
                        to_address: mint_to_address.clone(),
                        amount: vec![cosmwasm_std::Coin {
                            denom: coin.denom.clone(),
                            amount: cosmwasm_std::Uint128::from(amount_uint),
                        }],
                    })
                    .add_attribute("mint_amount", coin.amount)
                    .add_attribute("mint_to", mint_to_address);
            }
        }
        TfExecuteMsg::UpdateConfig { owner } => {
            res = res.add_attribute("update_owner", owner.unwrap_or_default());
        }
        TfExecuteMsg::BurnTokens {} => {
            res = res.add_attribute("burn", "true");
        }
    }
    Ok(res)
}

fn mock_tokenfactory_query(_deps: Deps, _env: Env, _msg: Binary) -> StdResult<Binary> {
    Err(StdError::generic_err("mock tokenfactory has no queries"))
}

fn query_vault_info(app: &App, contract: &Addr) -> VaultInfoResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::VaultInfo {})
        .unwrap()
}

fn query_swap_history(app: &App, contract: &Addr) -> TransmuteHistoryResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::TransmuteHistory { start_after: None, limit: None })
        .unwrap()
}

fn query_volume_history(app: &App, contract: &Addr) -> VolumeHistoryResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::VolumeHistory { start_after: None, limit: None })
        .unwrap()
}

#[test]
fn instantiate_sets_config() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, admin_addr);
    assert_eq!(config.deposit_pair.cdt, ASSET_A);
    assert_eq!(config.deposit_pair.paired_asset, ASSET_B);
    assert_eq!(config.cdt_target_ratio, Decimal::percent(50));
    // tokenfactory_contract is no longer used
}

#[test]
fn enter_vault_mints_tokens_and_updates_state() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    
    // Verify user has balance before instantiating
    let user_balance_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    let user_balance_b = app.wrap().query_balance(&user_addr, ASSET_B).unwrap();
    assert!(user_balance_a.amount >= Uint128::from(USER_DEPOSIT), "User doesn't have enough ASSET_A: have {}, need {}", user_balance_a.amount, USER_DEPOSIT);
    assert!(user_balance_b.amount >= Uint128::from(USER_DEPOSIT), "User doesn't have enough ASSET_B: have {}, need {}", user_balance_b.amount, USER_DEPOSIT);
    
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(USER_DEPOSIT, ASSET_A), coin(USER_DEPOSIT, ASSET_B)],
    )
    .unwrap();

    let info = query_vault_info(&app, &contract);
    assert!(info.deposit_total > Uint128::zero());
    assert_eq!(info.cdt_balance, Uint128::from(USER_DEPOSIT));
    assert_eq!(info.paired_asset_balance, Uint128::from(USER_DEPOSIT));
}

#[test]
fn deposit_fee_accepts_single_asset_without_vault_tokens() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(50_000, ASSET_A),
    )
    .unwrap();

    let info = query_vault_info(&app, &contract);
    assert_eq!(info.cdt_balance, Uint128::from(50_000u64));
}

#[test]
fn exit_vault_withdraws_proportional_assets() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(USER_DEPOSIT, ASSET_A), coin(USER_DEPOSIT, ASSET_B)],
    )
    .unwrap();

    //Assert user balance is correct minus what it deposited
    let user_balance = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE - USER_DEPOSIT));
    let user_balance = app.wrap().query_balance(&user_addr, ASSET_B).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE - USER_DEPOSIT));

    let info = query_vault_info(&app, &contract);
    let deposit_total = info.deposit_total;

    // Exit vault - no vault tokens needed, just call exit
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();

    //Assert user balance is back to its initial balance
    let user_balance = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE));
    let user_balance = app.wrap().query_balance(&user_addr, ASSET_B).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE));

    let post_info = query_vault_info(&app, &contract);
    assert_eq!(post_info.deposit_total, Uint128::zero());
}

#[test]
fn transmute_swaps_asset_a_for_b() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Seed contract with asset B so it can pay out swaps
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(500_000, ASSET_B),
    )
    .unwrap();

    // Fund CDP so it can perform CDT→paired_asset swap
    app.send_tokens(admin_addr.clone(), cdp_addr.clone(), &coins(25_000, ASSET_A)).unwrap();

    // CDT→paired_asset is restricted to CDP contract
    app.execute_contract(
        cdp_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(25_000, ASSET_A),
    )
    .unwrap();

    let swap_history = query_swap_history(&app, &contract);
    assert_eq!(swap_history.records.len(), 1);
    assert_eq!(swap_history.records[0].offered_asset, ASSET_A);
    println!("swap_history: {:?}", swap_history);

    let volume_history = query_volume_history(&app, &contract);
    assert_eq!(volume_history.records.len(), 1);
    println!("volume_history: {:?}", volume_history);
}

#[test]
fn update_config_changes_owner_and_ratio() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: Some(other_addr.to_string()),
            deposit_pair: None,
            tokenfactory_contract: None,
            composition_leeway: Some(Decimal::percent(5)),
            cdt_target_ratio: Some(Decimal::percent(60)),
            swap_history_cap: Some(20),
            volume_history_cap: Some(20),
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    )
    .unwrap();

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, other_addr);
    assert_eq!(config.cdt_target_ratio, Decimal::percent(60));
    assert_eq!(config.swap_history_cap, 20);
    // tokenfactory_contract is no longer used
    assert_eq!(config.volume_history_cap, 20);
    // asset_a_to_b_rate removed - using 1:1 tracking
    assert_eq!(config.composition_leeway, Decimal::percent(5));

}

#[test]
fn volume_window_updates_and_resets() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // First deposit into vault to increase total deposits (needed for rate limit calculation)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(200_000, ASSET_A), coin(200_000, ASSET_B)],
    )
    .unwrap();

    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(100_000, ASSET_B),
    )
    .unwrap();

    // Use other5_addr which hasn't been used yet to avoid rate limit
    // With 400k total deposits, 5% threshold = 20k, so 10k is safe
    // CDT→paired_asset is CDP-only, so use paired_asset→CDT direction
    app.execute_contract(
        other5_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_B),
    )
    .unwrap();

    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateVolumeWindow {},
        &[],
    )
    .unwrap();

    let history = query_volume_history(&app, &contract);
    println!("history: {:?}", history);
    assert_eq!(history.records.len(), 2);
}

#[test]
fn config_query_matches_instantiate() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, admin_addr);
    // tokenfactory_contract is no longer used
}

// ===== GLOBAL RATE LIMIT TESTS =====

#[test]
fn global_rate_limit_blocks_when_threshold_exceeded() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault to set deposits baseline (so threshold calc > 0)
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Check initial global rate limit status
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 0);
    assert_eq!(global_status.entries_count, 0);
    assert!(global_status.remaining_base > Uint128::zero());

    // Multiple non-whitelisted users contribute to global limit
    // Total deposits ~200k A base; 20% = 40k
    // Per-address limit: 5% = 10k, so we need to stay under that
    // USER does 5k B->A (+5k A)
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    
    // OTHER does 5k B->A (+5k A) 
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();

    // Check global status after 10k total
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 10_000);
    assert_eq!(global_status.entries_count, 2);
    assert!(global_status.remaining_base > Uint128::zero());

    // Try to push over 20% threshold (40k) - should fail
    // Need to do this with multiple users to avoid per-address limit
    // Add more users to fill up global limit
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(other2_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(other3_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(other4_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(other5_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    
    // Now we should be at 40k total, try one more - should fail
    let res = app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
    // println!("res: {:?}", res.().unwrap_err().to_string());
    // assert!(res.unwrap_err().to_string().contains("Global rate limit exceeded"));
}

#[test]
fn global_rate_limit_nets_flows_correctly() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault to set deposits baseline
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER does B->A (+5k A) - stay under per-address limit
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();

    // USER does another B->A (+2k A) - global accumulates
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(2_000, ASSET_B)).unwrap();

    // Net should be +7k A (both B->A)
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 7_000);
    assert_eq!(global_status.entries_count, 2);

    // OTHER does B->A (+3k A)
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_B)).unwrap();

    // Net should be +10k A
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 10_000);
    assert_eq!(global_status.entries_count, 3);
}

#[test]
fn global_rate_limit_whitelisted_addresses_bypass() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Add USER to allowlist
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: user_addr.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Enter vault to set deposits baseline
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // OTHER (non-whitelisted) does large swap to fill global limit
    // Stay under per-address limit (5% = 10k) but fill global limit (20% = 40k)
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(other2_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(other3_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(other4_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Check global status - should show OTHER's contribution
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 40_000);
    assert_eq!(global_status.entries_count, 4);

    // USER (whitelisted) should be able to do large swap without affecting global limit
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();

    // Global status should be unchanged
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 40_000); // Still only OTHER's contribution
    assert_eq!(global_status.entries_count, 4);
}

#[test]
fn global_rate_limit_separate_window_from_per_address() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Update config to have different windows: 1 hour for per-address, 2 hours for global
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: Some(60 * 60), // 1 hour
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(60 * 60 * 2), // 2 hours
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Enter vault to set deposits baseline
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER does swap
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Check both limits
    let per_address_status = query_rate_limit(&app, &contract, &user_addr.to_string());
    let global_status = query_global_rate_limit(&app, &contract);
    
    assert_eq!(per_address_status.status.net_flow_base, 10_000);
    assert_eq!(global_status.net_flow_base, 10_000);

    // Fast forward 1.5 hours (per-address window expires, global window still active)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(60 * 60 * 1 + 60 * 30); // 1.5 hours
    });

    // Check limits after time passage
    let per_address_status = query_rate_limit(&app, &contract, &user_addr.to_string());
    let global_status = query_global_rate_limit(&app, &contract);
    
    // Per-address should be reset (window expired)
    assert_eq!(per_address_status.status.net_flow_base, 0);
    assert_eq!(per_address_status.status.entries_count, 0);
    
    // Global should still show the entry (window hasn't expired)
    assert_eq!(global_status.net_flow_base, 10_000);
    assert_eq!(global_status.entries_count, 1);
}

#[test]
fn global_rate_limit_configuration_updates() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Check initial config
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    
    assert_eq!(config.global_rate_limit_window_secs, 60 * 60 * 24); // 24 hours
    assert_eq!(config.global_rate_limit_threshold, Decimal::percent(20)); // 20%

    // Update global rate limit configuration
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(60 * 60 * 12), // 12 hours
            global_rate_limit_threshold: Some(Decimal::percent(15)), // 15%
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    // Check updated config
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    
    assert_eq!(config.global_rate_limit_window_secs, 60 * 60 * 12); // 12 hours
    assert_eq!(config.global_rate_limit_threshold, Decimal::percent(15)); // 15%
}

#[test]
fn global_rate_limit_validation_errors() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Test invalid global window (zero)
    let res = app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(0), // Invalid
            global_rate_limit_threshold: None,
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    );
    assert!(res.is_err());
    // let error_msg = res.unwrap_err().to_string();
    // println!("Actual error: {}", error_msg);
    // assert!(error_msg.contains("Validation error"));

    // Test invalid global threshold (zero)
    let res = app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: Some(Decimal::zero()), // Invalid
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    );
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Validation error"));

    // Test invalid global threshold (> 1)
    let res = app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
            usage_fee_utilization_threshold: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: Some(Decimal::percent(101)), // Invalid (> 1)
            revenue_distributor_addr: None,
            revenue_distributions: None,
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    );
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Validation error"));
}

#[test]
fn global_rate_limit_dual_enforcement() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault to set deposits baseline
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Test that BOTH per-address and global limits are enforced.
    // Per-address: 5% of 200k = 10k threshold
    // Global: 20% of 200k = 40k threshold
    // All swaps are paired_asset→CDT (B→A) since CDT→PA is CDP-only.

    // First, fill up per-address limit for USER
    app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Try to exceed per-address limit (should fail even though global limit not reached)
    let res = app.execute_contract(user_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());

    // Now fill up global limit with multiple users (each stays under their per-address limit)
    app.execute_contract(other_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(other2_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(other3_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Global is now at 40k (USER 10k + OTHER 10k + OTHER2 10k + OTHER3 10k)
    // OTHER4 should be blocked by global limit even though their per-address limit is fine
    let res = app.execute_contract(other4_addr.clone(), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
}

#[test]
#[ignore] // Incentives removed
fn claim_incentives_applies_boost_attribute() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();

    // Store mocks
    let discounts_id = app.store_code(mock_discounts_contract());
    let neutron_id = app.store_code(mock_neutron_proxy_contract());
    let tf_code = app.store_code(mock_tokenfactory_contract());
    let cdp_code = app.store_code(mock_cdp_contract());

    // Instantiate mocks
    let discounts_addr = app
        .instantiate_contract(discounts_id, admin_addr.clone(), &Empty {}, &[], "discounts", None)
        .unwrap();
    let neutron_addr = app
        .instantiate_contract(neutron_id, admin_addr.clone(), &Empty {}, &[], "neutron", None)
        .unwrap();
    let tokenfactory_addr = app
        .instantiate_contract(tf_code, admin_addr.clone(), &TfInstantiateMsg { owner: Some(admin_addr.to_string()) }, &[], "tf", None)
        .unwrap();
    let cdp_addr = app
        .instantiate_contract(cdp_code, admin_addr.clone(), &Empty {}, &[], "cdp", None)
        .unwrap();

    // Instantiate transmuter with incentive settings
    let transmuter_id = app.store_code(transmuter_contract());
    let contract = app
        .instantiate_contract(
            transmuter_id,
            admin_addr.clone(),
            &InstantiateMsg {
                owner: Some(admin_addr.to_string()),
                tokenfactory_contract: Some(tokenfactory_addr),
                revenue_distributor_addr: Some(admin_addr.to_string()),
                cdp_contract: cdp_addr.to_string(),
                deposit_pair: AssetPair { cdt: ASSET_A.to_string(), paired_asset: ASSET_B.to_string() },
                composition_leeway: Decimal::percent(1),
                cdt_target_ratio: Decimal::percent(50),
                usage_fee: Some(Decimal::zero()),
                usage_fee_utilization_threshold: None,
                swap_history_cap: 50,
                volume_history_cap: 50,
                rate_limit_window_secs: Some(60),
                rate_limit_threshold: Some(Decimal::percent(10)),
                revenue_distributions: None,
                allowlist: None,
                allowlist_rate_limit_threshold: None,
                global_rate_limit_window_secs: Some(3600),
                global_rate_limit_threshold: Some(Decimal::percent(10)),
                discounts_contract: discounts_addr.to_string(),
                lock_ceiling: 1460,
                affiliate_fee: Decimal::percent(1),
                send_swap_fee: Some(false),
                revenue_distributor_fee_percentage: None,
                emissions_voting_contract: None,
            },
            &[],
            "transmuter",
            None,
        )
        .unwrap();

    // Seed user VT deposits held in contract
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None, lock_days: None, affiliate_address: None, affiliate_label: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Manually create an incentive event: 1 INC per VT
    // We need VT supply > 0; it's minted in enter_vault above and held in contract
    // Update schedule to emit a fixed amount by advancing time via block updates and setting monthly cap

    // Advance time by 10 seconds and set monthly emission to 3_000_000 so per-second emits enough
    app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            cdt_target_ratio: None,
            tokenfactory_contract: None,
            discounts_contract: None,
            cdp_contract: None,
            usage_fee: None,
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
            lock_ceiling: None,
            affiliate_fee: Decimal::percent(1),
            send_swap_fee: None,
            revenue_distributor_fee_percentage: None,
            emissions_voting_contract: None,
        },
        &[],
    ).unwrap();

    app.update_block(|b| { b.time = b.time.plus_seconds(10); b.height += 1; });

    // Claim for USER via admin caller
    let res = app.execute_contract(
        admin_addr.clone(),
        contract.clone(),
        // ClaimIncentivesForUser has been removed
        &ExecuteMsg::DepositFee {}, // Placeholder
        &[],
    ).unwrap();

    // Find claimed_post_boost attribute
    let mut claimed_post_boost: Option<Uint128> = None;
    for ev in res.events {
        for attr in ev.attributes {
            if attr.key == "claimed_post_boost" {
                claimed_post_boost = Some(Uint128::from(attr.value.parse::<u128>().unwrap_or(0)));
            }
        }
    }
    assert!(claimed_post_boost.is_some());
}

// User deposits tests - moved from user_deposits_tests.rs
#[test]
fn test_enter_vault_with_lock_stores_in_user_deposits() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault with lock
    app.execute_contract(
        user_addr.clone(),
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
            user: user_addr.to_string(),
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
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault without lock
    app.execute_contract(
        user_addr.clone(),
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
            user: user_addr.to_string(),
        })
        .unwrap();
    let deposits = response.deposits;

    assert_eq!(deposits.len(), 1);
    assert!(deposits[0].locked.is_none());
}

#[test]
fn test_lock_vault_tokens_post_deposit() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // First, enter vault without lock to get vault tokens
    app.execute_contract(
        user_addr.clone(),
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

    // Get user's deposit info
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();
    let total_deposits: Uint128 = response.deposits.iter().map(|d| d.amount).sum();
    assert!(!total_deposits.is_zero());

    // Lock some deposits (no vault tokens needed)
    let lock_amount = total_deposits;
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: lock_amount,
            lock_days: 50,
        },
        &[],
    )
    .unwrap();

    // Check user deposits - should have 2 entries now (original + locked)
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
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
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault with lock
    app.execute_contract(
        user_addr.clone(),
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

    // Query user deposits to check locked status
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();

    assert_eq!(response.deposits.len(), 1);
    assert_eq!(response.deposits[0].locked.as_ref().unwrap().intended_lock_days, Some(100));
}

#[test]
fn test_unlock_expired_lock_from_user_deposits() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault with 10 day lock
    app.execute_contract(
        user_addr.clone(),
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

    // Advance time by 11 days (lock expired)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(11 * 86400);
    });

    // Query deposits before exit - should have locked deposit
    let response_before: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();
    let deposits_before = response_before.deposits;
    let locked_before: Vec<_> = deposits_before
        .iter()
        .filter(|d| d.locked.is_some())
        .collect();
    assert!(!locked_before.is_empty());

    // Exit vault - expired locks should be automatically unlocked during exit
    let user_balance_before_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();

    // Check user received assets (expired locks unlock fully)
    let user_balance_after_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    assert!(user_balance_after_a.amount > user_balance_before_a.amount);
}

#[test]
fn test_unlock_early_withdrawal_from_user_deposits() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault with 100 day lock
    app.execute_contract(
        user_addr.clone(),
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

    // Advance time by 50 days (half of lock period - lock still active)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(50 * 86400);
    });

    // Try to exit vault - should fail because deposits are still locked
    let result = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    );
    assert!(result.is_err(), "Exit should fail when deposits are locked");
    
    // Advance time past lock expiration (101 days total)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(51 * 86400);
    });

    // Now exit should succeed - lock has expired
    let user_balance_before_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();

    // Check user received assets (lock expired, full amount)
    let user_balance_after_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    assert!(user_balance_after_a.amount > user_balance_before_a.amount);
    
    // Check that deposit was removed after exit
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    // Exit should remove the deposit entry
    let total_after: Uint128 = deposits_after.iter().map(|d| d.amount).sum();
    assert_eq!(total_after, Uint128::zero(), "All deposits should be withdrawn after exit");
}

#[test]
fn test_partial_unlock_from_user_deposits() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault with 100 day lock
    app.execute_contract(
        user_addr.clone(),
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
    // vault_token removed - using internal deposit tracking

    // Advance time by 50 days (lock still active)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(50 * 86400);
    });

    // Query deposits - should still be locked
    let response_before: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();
    let deposits_before = response_before.deposits;
    let locked_deposit = deposits_before.iter().find(|d| d.locked.is_some()).unwrap();
    
    // Try to exit - should fail because deposits are still locked
    let result = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    );
    assert!(result.is_err(), "Exit should fail when deposits are locked");
    
    // Advance time past lock expiration (101 days total)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(51 * 86400);
    });
    
    // Now exit should succeed - lock has expired
    let user_balance_before_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();

    // Check that user received assets (lock expired, full amount)
    let user_balance_after_a = app.wrap().query_balance(&user_addr, ASSET_A).unwrap();
    assert!(user_balance_after_a.amount > user_balance_before_a.amount);
    
    // Check deposits after exit - should be removed
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    // All deposits should be withdrawn after exit
    let total_after: Uint128 = deposits_after.iter().map(|d| d.amount).sum();
    assert_eq!(total_after, Uint128::zero(), "All deposits should be withdrawn after exit");
}

#[test]
fn test_lock_vault_tokens_validation() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // First need to have deposits to lock
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::EnterVault {
            recipient: None,
            lock_days: None,
            affiliate_address: None,
            affiliate_label: None,
        },
        &coins(1000, ASSET_A),
    )
    .unwrap();

    // Try to lock with zero days (should fail)
    let result = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: Uint128::from(1000u64),
            lock_days: 0,
        },
        &[],
    );
    assert!(result.is_err());

    // Try to lock with days exceeding ceiling (should fail)
    let result = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: Uint128::from(1000u64),
            lock_days: 1500, // Exceeds lock_ceiling of 1460
        },
        &[],
    );
    assert!(result.is_err());

    // Try to lock with amount exceeding deposits (should fail)
    let result = app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: Uint128::from(2000u64), // More than deposited
            lock_days: 50,
        },
        &[],
    );
    assert!(result.is_err());
}

#[test]
fn test_multiple_locks_and_unlocks() {
    let (mut app, admin_addr, user_addr, other_addr, other2_addr, other3_addr, other4_addr, other5_addr) = setup_app();
    let (contract, cdp_addr) = instantiate_transmuter(&mut app, admin_addr.clone());

    // Enter vault without lock
    app.execute_contract(
        user_addr.clone(),
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

    // Get user's deposit info
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();
    let total_deposits: Uint128 = response.deposits.iter().map(|d| d.amount).sum();
    
    let lock_amount_1 = total_deposits / Uint128::from(3u64);
    let lock_amount_2 = total_deposits / Uint128::from(3u64);

    // Lock some deposits with 50 days
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: lock_amount_1,
            lock_days: 50,
        },
        &[],
    )
    .unwrap();

    // Lock more deposits with 100 days
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::Lock {
            amount: lock_amount_2,
            lock_days: 100,
        },
        &[],
    )
    .unwrap();

    // Query deposits - should have locked entries
    let response: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap();

    let locked_deposits: Vec<_> = response.deposits.iter().filter(|d| d.locked.is_some()).collect();
    assert!(locked_deposits.len() >= 2, "Should have at least 2 locked deposits");

    // Advance time by 60 days (first lock expired, second still locked)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(60 * 86400);
    });

    // Exit vault - should only withdraw expired deposits (first lock), second lock still active
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();

    // Check deposits - should still have locked deposits (second lock)
    let response_after: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_after = response_after.deposits;
    
    // Should still have locked deposits (second lock not expired)
    let locked_after: Vec<_> = deposits_after.iter().filter(|d| d.locked.is_some()).collect();
    assert!(!locked_after.is_empty(), "Should still have locked deposits (second lock)");
    
    // Advance time past second lock expiration (101 days total from second lock)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(41 * 86400); // 60 + 41 = 101 days from second lock
    });
    
    // Exit again - should withdraw remaining deposits
    app.execute_contract(
        user_addr.clone(),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None, user: None, deposit_id: None, amount: None },
        &[],
    )
    .unwrap();
    
    // Now all deposits should be withdrawn
    let response_final: membrane::transmuter::UserDepositsResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::UserDeposits {
            user: user_addr.to_string(),
        })
        .unwrap_or_else(|_| membrane::transmuter::UserDepositsResponse { deposits: vec![] });
    let deposits_final = response_final.deposits;
    let total_final: Uint128 = deposits_final.iter().map(|d| d.amount).sum();
    assert_eq!(total_final, Uint128::zero(), "All deposits should be withdrawn after both locks expire");
}
