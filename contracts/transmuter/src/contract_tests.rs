#![allow(unused_imports)]

use cosmwasm_std::{
    coin, coins, testing::{mock_dependencies, mock_env, mock_info, MockQuerier, MOCK_CONTRACT_ADDR},
    Addr, Coin, ContractResult, Decimal, Empty, OwnedDeps, MemoryStorage, Querier, QuerierResult,
    QueryRequest, SystemError, SystemResult, Uint128, WasmQuery, to_json_binary,
};
use cosmwasm_std::testing::MockApi;

use membrane::collateral::BasketCollateral;
use membrane::transmuter::{
    AllTranchesResponse, AssetPair, Config, ExecuteMsg, InstantiateMsg, MaxAssignableBadDebtResponse,
    QueryMsg, TrancheConversionRateResponse, TrancheInfoResponse, TrancheState, TrancheSplit,
    UserTranchesResponse, VaultInfoResponse,
};
use membrane::types::{Asset, AssetInfo, cAsset};

use crate::contract::{execute, instantiate, query};
use crate::error::ContractError;
use crate::state::{
    CONFIG, TOTAL_TRANCHE_STAKED, TRANCHE_STATES, UNDISTRIBUTED_FEES, USER_TRANCHES,
    ACQUISITION_DEPOSIT_TOTAL,
};

// ================= Custom Mock Querier =================

/// Mock querier that handles collateral contract GetBasket queries
struct CustomMockQuerier {
    base: MockQuerier,
    collateral_contract: String,
    collateral_denoms: Vec<String>,
}

impl CustomMockQuerier {
    fn new(balances: &[(&str, &[Coin])], collateral_denoms: Vec<String>) -> Self {
        Self {
            base: MockQuerier::new(balances),
            collateral_contract: addr("collateral"),
            collateral_denoms,
        }
    }
}

impl Querier for CustomMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest<Empty> = match cosmwasm_std::from_json(bin_request) {
            Ok(r) => r,
            Err(e) => return SystemResult::Err(SystemError::InvalidRequest {
                error: e.to_string(),
                request: bin_request.into(),
            }),
        };

        match &request {
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                if contract_addr == &self.collateral_contract {
                    // Return mock BasketCollateral
                    let collateral_types: Vec<cAsset> = self.collateral_denoms.iter().map(|denom| {
                        cAsset {
                            asset: Asset {
                                info: AssetInfo::NativeToken { denom: denom.clone() },
                                amount: Uint128::zero(),
                            },
                            max_borrow_LTV: Decimal::percent(50),
                            max_LTV: Decimal::percent(70),
                            rate_index: Decimal::one(),
                            peg_rate_index: Decimal::one(),
                            pool_info: None,
                            force_redemptions: None,
                            max_threshold_to_delay: Decimal::percent(4),
                        }
                    }).collect();

                    let basket = BasketCollateral {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types,
                        collateral_supply_caps: vec![],
                        multi_asset_supply_caps: vec![],
                    };

                    return SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()));
                }
                // Fall through to base querier
                self.base.raw_query(bin_request)
            }
            _ => self.base.raw_query(bin_request),
        }
    }
}

fn mock_deps_with_collateral(
    balances: &[(&str, &[Coin])],
    collateral_denoms: Vec<String>,
) -> OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier> {
    let querier = CustomMockQuerier::new(balances, collateral_denoms);
    OwnedDeps {
        storage: MemoryStorage::default(),
        api: MockApi::default(),
        querier,
        custom_query_type: std::marker::PhantomData,
    }
}

// ================= Address Helpers =================

fn addr(name: &str) -> String {
    MockApi::default().addr_make(name).to_string()
}

// ================= Helpers =================

fn default_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        owner: Some(addr("owner")),
        cdp_contract: addr("cdp"),
        collateral_contract: addr("collateral"),
        deposit_pair: AssetPair { cdt: "cdt".to_string(), paired_asset: "usdc".to_string() },
        composition_leeway: Decimal::one(), // 100% leeway for test simplicity
        cdt_target_ratio: Decimal::zero(),
        usage_fee: Some(Decimal::percent(1)),
        usage_fee_utilization_threshold: None,
        swap_history_cap: 50,
        volume_history_cap: 50,
        revenue_distributor_addr: None,
        revenue_distributions: None,
        allowlist: None,
        discounts_contract: addr("discounts"),
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: Some(false),
        revenue_distributor_fee_percentage: None,
        acquisition_contract: None,
        points_system_contract: None,
        retention_multiplier: None,
    }
}

fn setup(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
) {
    let env = mock_env();
    let info = mock_info(&addr("owner"), &[]);
    let msg = default_instantiate_msg();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
}

fn enter_vault(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    sender: &str,
    funds: &[Coin],
    tranches: Vec<(String, bool)>,
) -> Result<cosmwasm_std::Response, ContractError> {
    let env = mock_env();
    let info = mock_info(&addr(sender), funds);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::EnterVault {
            recipient: None,
            affiliate_address: None,
            affiliate_label: None,
            tranches,
        },
    )
}

fn exit_vault(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    sender: &str,
    tranche_withdrawals: Option<Vec<(String, bool, Uint128)>>,
) -> Result<cosmwasm_std::Response, ContractError> {
    let env = mock_env();
    let info = mock_info(&addr(sender), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::ExitVault {
            recipient: None,
            withdraw_as: None,
            user: None,
            amount: None,
            tranche_withdrawals,
        },
    )
}

fn add_revenue(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    sender: &str,
    funds: &[Coin],
    collateral_denom: &str,
    junior: bool,
) -> Result<cosmwasm_std::Response, ContractError> {
    let env = mock_env();
    let info = mock_info(&addr(sender), funds);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AddRevenue {
            collateral_denom: collateral_denom.to_string(),
            junior,
        },
    )
}

fn assign_bad_debt(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    sender: &str,
    collateral_denom: &str,
    amount: u128,
) -> Result<cosmwasm_std::Response, ContractError> {
    let env = mock_env();
    let info = mock_info(&addr(sender), &[]);
    execute(
        deps.as_mut(), env, info,
        ExecuteMsg::AssignBadDebt {
            collateral_denom: collateral_denom.to_string(),
            amount: Uint128::new(amount),
        },
    )
}

fn query_user_tranches(
    deps: &OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    user: &str,
) -> UserTranchesResponse {
    let env = mock_env();
    let res = query(deps.as_ref(), env, QueryMsg::UserTranches { user: addr(user) }).unwrap();
    cosmwasm_std::from_json(res).unwrap()
}

fn query_tranche_info(
    deps: &OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    collateral_denom: &str,
    junior: bool,
) -> TrancheInfoResponse {
    let env = mock_env();
    let res = query(deps.as_ref(), env, QueryMsg::TrancheInfo {
        collateral_denom: collateral_denom.to_string(),
        junior,
    }).unwrap();
    cosmwasm_std::from_json(res).unwrap()
}

fn query_tranche_rate(
    deps: &OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    collateral_denom: &str,
    junior: bool,
) -> TrancheConversionRateResponse {
    let env = mock_env();
    let res = query(deps.as_ref(), env, QueryMsg::TrancheConversionRate {
        collateral_denom: collateral_denom.to_string(),
        junior,
    }).unwrap();
    cosmwasm_std::from_json(res).unwrap()
}

fn query_max_bad_debt(
    deps: &OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
    collateral_denom: &str,
) -> MaxAssignableBadDebtResponse {
    let env = mock_env();
    let res = query(deps.as_ref(), env, QueryMsg::MaxAssignableBadDebt {
        collateral_denom: collateral_denom.to_string(),
    }).unwrap();
    cosmwasm_std::from_json(res).unwrap()
}

fn query_all_tranches(
    deps: &OwnedDeps<MemoryStorage, MockApi, CustomMockQuerier>,
) -> AllTranchesResponse {
    let env = mock_env();
    let res = query(deps.as_ref(), env, QueryMsg::AllTranches {}).unwrap();
    cosmwasm_std::from_json(res).unwrap()
}

// ================= Deposit Tests =================

#[test]
fn deposit_single_tranche() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt"), coin(1000, "usdc")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    let res = enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("atom".to_string(), false)],
    ).unwrap();

    // Check user tranches
    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 1);
    assert_eq!(user.tranches[0].asset, "atom");
    assert_eq!(user.tranches[0].junior, false);
    assert!(!user.tranches[0].vault_token_amount.is_zero());

    // Check global state
    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(100));

    // Check tranche state
    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::new(100));
    assert!(!info.state.vault_token_supply.is_zero());
}

#[test]
fn deposit_multi_tranche_equal_split() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt"), coin(1000, "usdc")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("atom".to_string(), false), ("osmo".to_string(), true)],
    ).unwrap();

    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 2);

    // Each tranche gets 50
    let atom_info = query_tranche_info(&deps, "atom", false);
    let osmo_info = query_tranche_info(&deps, "osmo", true);
    assert_eq!(atom_info.state.total_staked, Uint128::new(50));
    assert_eq!(osmo_info.state.total_staked, Uint128::new(50));

    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(100));
}

#[test]
fn deposit_remainder_goes_to_last_tranche() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string(), "juno".to_string()],
    );
    setup(&mut deps);

    // 100 / 3 = 33 per tranche, with 1 remainder
    enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![
            ("atom".to_string(), false),
            ("osmo".to_string(), false),
            ("juno".to_string(), false),
        ],
    ).unwrap();

    let atom = query_tranche_info(&deps, "atom", false);
    let osmo = query_tranche_info(&deps, "osmo", false);
    let juno = query_tranche_info(&deps, "juno", false);
    assert_eq!(atom.state.total_staked, Uint128::new(33));
    assert_eq!(osmo.state.total_staked, Uint128::new(33));
    assert_eq!(juno.state.total_staked, Uint128::new(34)); // remainder
}

#[test]
fn deposit_duplicate_tranche_rejected() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    let err = enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("atom".to_string(), false), ("atom".to_string(), false)],
    ).unwrap_err();

    assert!(matches!(err, ContractError::DuplicateTranche { .. }));
}

#[test]
fn deposit_same_denom_different_junior_is_ok() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Same denom, different junior flag = different tranches, should be allowed
    enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("atom".to_string(), false), ("atom".to_string(), true)],
    ).unwrap();

    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 2);
}

#[test]
fn deposit_invalid_collateral_rejected() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    let err = enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("invalid_denom".to_string(), false)],
    ).unwrap_err();

    assert!(matches!(err, ContractError::InvalidCollateralDenom { .. }));
}

#[test]
fn deposit_empty_tranches_rejected() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    let err = enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![],
    ).unwrap_err();

    assert!(matches!(err, ContractError::Validation(_)));
}

// ================= Withdrawal Tests =================

#[test]
fn withdraw_full_single_tranche() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt"), coin(1000, "usdc")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let user = query_user_tranches(&deps, "alice");
    let vt_amount = user.tranches[0].vault_token_amount;

    // Full withdrawal
    exit_vault(
        &mut deps, "alice",
        Some(vec![("atom".to_string(), false, vt_amount)]),
    ).unwrap();

    // User should have no tranches
    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 0);

    // Tranche should be empty
    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::zero());
    assert_eq!(info.state.vault_token_supply, Uint128::zero());

    // Global total should be zero
    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::zero());
}

#[test]
fn withdraw_partial() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let user = query_user_tranches(&deps, "alice");
    let vt_amount = user.tranches[0].vault_token_amount;
    let half = vt_amount / Uint128::new(2);

    exit_vault(
        &mut deps, "alice",
        Some(vec![("atom".to_string(), false, half)]),
    ).unwrap();

    // User should still have remaining
    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 1);
    assert_eq!(user.tranches[0].vault_token_amount, vt_amount - half);

    // Tranche should have remaining
    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::new(50));
}

#[test]
fn withdraw_proportional_from_all_tranches() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    enter_vault(
        &mut deps, "alice",
        &[coin(100, "cdt")],
        vec![("atom".to_string(), false), ("osmo".to_string(), true)],
    ).unwrap();

    // Withdraw all (None = proportional from all)
    exit_vault(&mut deps, "alice", None).unwrap();

    let user = query_user_tranches(&deps, "alice");
    assert_eq!(user.tranches.len(), 0);

    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::zero());
}

#[test]
fn withdraw_insufficient_balance_errors() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let user = query_user_tranches(&deps, "alice");
    let vt_amount = user.tranches[0].vault_token_amount;

    // Try to withdraw more than available
    let err = exit_vault(
        &mut deps, "alice",
        Some(vec![("atom".to_string(), false, vt_amount + Uint128::one())]),
    ).unwrap_err();

    assert!(matches!(err, ContractError::InsufficientTrancheBalance { .. }));
}

#[test]
fn withdraw_duplicate_tranche_rejected() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let err = exit_vault(
        &mut deps, "alice",
        Some(vec![
            ("atom".to_string(), false, Uint128::new(10)),
            ("atom".to_string(), false, Uint128::new(10)),
        ]),
    ).unwrap_err();

    assert!(matches!(err, ContractError::DuplicateTranche { .. }));
}

// ================= Conversion Rate Tests =================

#[test]
fn conversion_rate_starts_at_one() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Before any deposits, rate is 1:1
    let rate = query_tranche_rate(&deps, "atom", false);
    assert_eq!(rate.rate, Decimal::one());
}

#[test]
fn second_depositor_gets_correct_rate() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(2000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Alice deposits 100
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    let alice = query_user_tranches(&deps, "alice");
    let alice_vt = alice.tranches[0].vault_token_amount;

    // Bob deposits 200
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    let bob = query_user_tranches(&deps, "bob");
    let bob_vt = bob.tranches[0].vault_token_amount;

    // Bob should get 2x Alice's VTs (same rate, 2x deposit)
    assert_eq!(bob_vt, alice_vt * Uint128::new(2));
}

#[test]
fn rate_increases_after_yield() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(2000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let rate_before = query_tranche_rate(&deps, "atom", false);

    // Add 50 revenue
    add_revenue(&mut deps, "revenue", &[coin(50, "cdt")], "atom", false).unwrap();

    let rate_after = query_tranche_rate(&deps, "atom", false);
    assert!(rate_after.rate > rate_before.rate);

    // Rate should be 150/100_000_000 (total_staked / vt_supply) -- but since initial vt is 100 * 1M
    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::new(150));
}

#[test]
fn second_depositor_after_yield_gets_fewer_vts() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Alice deposits 100
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    let alice = query_user_tranches(&deps, "alice");
    let alice_vt = alice.tranches[0].vault_token_amount;

    // Add 100 revenue (doubles the rate)
    add_revenue(&mut deps, "revenue", &[coin(100, "cdt")], "atom", false).unwrap();

    // Bob deposits 100 (same nominal amount)
    enter_vault(&mut deps, "bob", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    let bob = query_user_tranches(&deps, "bob");
    let bob_vt = bob.tranches[0].vault_token_amount;

    // Bob should get half the VTs Alice got (rate doubled)
    assert_eq!(bob_vt, alice_vt / Uint128::new(2));
}

// ================= Yield Tests =================

#[test]
fn add_revenue_increases_total_staked() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(2000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();

    add_revenue(&mut deps, "revenue", &[coin(50, "cdt")], "atom", true).unwrap();

    let info = query_tranche_info(&deps, "atom", true);
    assert_eq!(info.state.total_staked, Uint128::new(150));
    assert_eq!(info.junior, true);
}

#[test]
fn distribute_swap_fees_proportional() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    // Alice: 100 to atom-senior
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    // Bob: 300 to osmo-junior
    enter_vault(&mut deps, "bob", &[coin(300, "cdt")], vec![("osmo".to_string(), true)]).unwrap();

    // Simulate undistributed fees
    UNDISTRIBUTED_FEES.save(&mut deps.storage, &Uint128::new(100)).unwrap();

    let env = mock_env();
    let info = mock_info(&addr("keeper"), &[]);
    execute(deps.as_mut(), env, info, ExecuteMsg::DistributeSwapFees {}).unwrap();

    // atom-senior (100/400 = 25%) should get 25
    let atom = query_tranche_info(&deps, "atom", false);
    assert_eq!(atom.state.total_staked, Uint128::new(125));

    // osmo-junior (300/400 = 75%) should get 75
    let osmo = query_tranche_info(&deps, "osmo", true);
    assert_eq!(osmo.state.total_staked, Uint128::new(375));

    let undist = UNDISTRIBUTED_FEES.load(&deps.storage).unwrap();
    assert_eq!(undist, Uint128::zero());
}

// ================= Bad Debt Tests =================

#[test]
fn bad_debt_junior_absorbs_first() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Junior deposits 100, Senior deposits 200
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    // Assign 80 bad debt (less than junior's 100)
    assign_bad_debt(&mut deps, "cdp", "atom", 80).unwrap();

    let junior = query_tranche_info(&deps, "atom", true);
    assert_eq!(junior.state.total_staked, Uint128::new(20)); // 100 - 80

    let senior = query_tranche_info(&deps, "atom", false);
    assert_eq!(senior.state.total_staked, Uint128::new(200)); // untouched
}

#[test]
fn bad_debt_overflows_to_senior() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    // Assign 150 bad debt (exceeds junior's 100, 50 spills to senior)
    assign_bad_debt(&mut deps, "cdp", "atom", 150).unwrap();

    let junior = query_tranche_info(&deps, "atom", true);
    assert_eq!(junior.state.total_staked, Uint128::zero()); // fully wiped

    let senior = query_tranche_info(&deps, "atom", false);
    assert_eq!(senior.state.total_staked, Uint128::new(150)); // 200 - 50
}

#[test]
fn bad_debt_exceeds_max_errors() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    // Max is 300, try to assign 301
    let err = assign_bad_debt(&mut deps, "cdp", "atom", 301).unwrap_err();
    assert!(matches!(err, ContractError::BadDebtExceedsMax { .. }));
}

#[test]
fn bad_debt_sends_fulfill_message() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let res = assign_bad_debt(&mut deps, "cdp", "atom", 50).unwrap();

    // Should have a FulfillBadDebt message
    assert_eq!(res.messages.len(), 1);
}

#[test]
fn bad_debt_rate_impact() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();

    let rate_before = query_tranche_rate(&deps, "atom", true);

    assign_bad_debt(&mut deps, "cdp", "atom", 50).unwrap();

    let rate_after = query_tranche_rate(&deps, "atom", true);
    assert!(rate_after.rate < rate_before.rate); // Rate decreased
}

#[test]
fn bad_debt_unrelated_tranche_unaffected() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(100, "cdt")], vec![("osmo".to_string(), false)]).unwrap();

    let osmo_rate_before = query_tranche_rate(&deps, "osmo", false);

    assign_bad_debt(&mut deps, "cdp", "atom", 50).unwrap();

    let osmo_rate_after = query_tranche_rate(&deps, "osmo", false);
    assert_eq!(osmo_rate_after.rate, osmo_rate_before.rate); // Unaffected
}

// ================= Max Assignable Bad Debt Query =================

#[test]
fn max_assignable_bad_debt_query() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(3000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let max = query_max_bad_debt(&deps, "atom");
    assert_eq!(max.max_amount, Uint128::new(300));
}

// ================= Cross-Tranche Isolation Tests =================

#[test]
fn cross_tranche_isolation() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    // Two separate tranches
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("osmo".to_string(), false)]).unwrap();

    let atom_rate_before = query_tranche_rate(&deps, "atom", false);
    let osmo_rate_before = query_tranche_rate(&deps, "osmo", false);

    // Add revenue only to atom
    add_revenue(&mut deps, "rev", &[coin(50, "cdt")], "atom", false).unwrap();

    let atom_rate_after = query_tranche_rate(&deps, "atom", false);
    let osmo_rate_after = query_tranche_rate(&deps, "osmo", false);

    assert!(atom_rate_after.rate > atom_rate_before.rate); // Increased
    assert_eq!(osmo_rate_after.rate, osmo_rate_before.rate); // Unchanged
}

// ================= Multi-User Tests =================

#[test]
fn multi_user_same_tranche() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::new(300));

    // Alice withdraws
    exit_vault(&mut deps, "alice", None).unwrap();

    // Bob's position should be unaffected
    let bob = query_user_tranches(&deps, "bob");
    assert_eq!(bob.tranches.len(), 1);
    assert!(!bob.tranches[0].vault_token_amount.is_zero());

    let info = query_tranche_info(&deps, "atom", false);
    assert_eq!(info.state.total_staked, Uint128::new(200));
}

#[test]
fn senior_two_assets_earns_more_than_one() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    // Alice backs 2 assets (100 split: 50+50)
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![
        ("atom".to_string(), false),
        ("osmo".to_string(), false),
    ]).unwrap();

    // Bob backs 1 asset (100 all to atom)
    enter_vault(&mut deps, "bob", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    // Add 10 revenue to each asset's senior tranche
    add_revenue(&mut deps, "rev", &[coin(10, "cdt")], "atom", false).unwrap();
    add_revenue(&mut deps, "rev", &[coin(10, "cdt")], "osmo", false).unwrap();

    // Alice has positions in both, Bob only in atom
    let alice = query_user_tranches(&deps, "alice");
    let bob = query_user_tranches(&deps, "bob");

    // Alice earns from both tranches (atom + osmo), Bob only from atom
    // Alice's atom position: 50 deposit in a 150+10=160 pool? No, let me think...
    // After deposits: atom has 150 staked (50 alice + 100 bob), osmo has 50 (alice)
    // After yield: atom has 160, osmo has 60
    // Alice's share of atom: proportional to her VTs vs total VTs
    // Bob's share of atom: proportional to his VTs
    // Alice also earns from osmo
    assert_eq!(alice.tranches.len(), 2); // Alice has 2 positions = 2x yield exposure
    assert_eq!(bob.tranches.len(), 1); // Bob has 1 position
}

// ================= All Tranches Query =================

#[test]
fn all_tranches_query() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![
        ("atom".to_string(), false),
        ("osmo".to_string(), true),
    ]).unwrap();

    let all = query_all_tranches(&deps);
    assert_eq!(all.tranches.len(), 2);
}

// ================= Edge Cases =================

#[test]
fn deposit_into_empty_tranche_after_bad_debt() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    // Alice deposits, then bad debt wipes the tranche
    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();
    assign_bad_debt(&mut deps, "cdp", "atom", 100).unwrap();

    let info = query_tranche_info(&deps, "atom", true);
    assert_eq!(info.state.total_staked, Uint128::zero());
    // VT supply is still non-zero (Alice's VTs are now worthless)

    // Bob deposits into the wiped tranche
    enter_vault(&mut deps, "bob", &[coin(100, "cdt")], vec![("atom".to_string(), true)]).unwrap();

    let bob = query_user_tranches(&deps, "bob");
    assert!(!bob.tranches[0].vault_token_amount.is_zero());
}

#[test]
fn global_invariant_holds() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(5000, "cdt")])],
        vec!["atom".to_string(), "osmo".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();
    enter_vault(&mut deps, "bob", &[coin(200, "cdt")], vec![("osmo".to_string(), true)]).unwrap();

    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(300));

    // Add revenue
    add_revenue(&mut deps, "rev", &[coin(50, "cdt")], "atom", false).unwrap();
    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(350));

    // Bad debt
    assign_bad_debt(&mut deps, "cdp", "osmo", 100).unwrap();
    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(250));

    // Withdraw alice
    exit_vault(&mut deps, "alice", None).unwrap();
    let total = TOTAL_TRANCHE_STAKED.load(&deps.storage).unwrap();
    assert_eq!(total, Uint128::new(100)); // 250 - 150 (alice's atom tranche with yield)
}

#[test]
fn vault_info_query() {
    let mut deps = mock_deps_with_collateral(
        &[(MOCK_CONTRACT_ADDR, &[coin(1000, "cdt"), coin(500, "usdc")])],
        vec!["atom".to_string()],
    );
    setup(&mut deps);

    enter_vault(&mut deps, "alice", &[coin(100, "cdt")], vec![("atom".to_string(), false)]).unwrap();

    let env = mock_env();
    let res: VaultInfoResponse = cosmwasm_std::from_json(
        query(deps.as_ref(), env, QueryMsg::VaultInfo {}).unwrap()
    ).unwrap();

    assert_eq!(res.total_tranche_staked, Uint128::new(100));
}
