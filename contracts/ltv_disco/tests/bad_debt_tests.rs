use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{coins, from_json, Coin, CosmosMsg, Decimal, Uint128, WasmMsg, to_json_binary, OwnedDeps, MemoryStorage, ReplyOn};
use membrane::ltv_disco::{BackingDepositInput, Config, ExecuteMsg, InstantiateMsg, LTVQueue};
use membrane::types::DepositDenom;
use membrane::oracle::{QueryMsg as Oracle_QueryMsg, PriceResponse};
use membrane::osmosis_proxy::ExecuteMsg as OsmosisProxy_ExecuteMsg;
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;

use ltv_disco::contract::{execute, instantiate, LIQUIDATION_SWAP_REPLY_ID};
use ltv_disco::state::{CONFIG, DISPERSAL, LTV_QUEUES, SWAP_PROPAGATION};
use membrane::ltv_disco::{Dispersal, ActiveDispersal};

/// Helper to create a standard config for testing
fn create_test_config(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
let msg = InstantiateMsg {
    owner: Some("owner".to_string()),
    cdp_contract: "cdp_contract".to_string(),
    deposit_denom: DepositDenom {
        denom: "collateral".to_string(),
        vault_info: None,
    },
    cdt_denom: "cdt".to_string(),
    minimum_deposit: Uint128::new(1000),
    max_ltv: Decimal::percent(95),
    percent_to_disperse: Decimal::percent(10),
    dispersal_window: 24,
    activation_window: 48,
    oracle_contract: "oracle".to_string(),
    chain_proxy_contract: "chain_proxy".to_string(),
    lock_duration_ceiling: Some(365),
    affiliate_fee: Some(Decimal::percent(1)),
    max_management_fee: None,
    ltv_delta_minimum: Some(Decimal::percent(1)),
    emissions_voting_contract: None,
    points_system_contract: None,
    revenue_distributor: None,
};

let info = mock_info("creator", &[]);
let env = mock_env();
instantiate(deps.as_mut(), env, info, msg).unwrap();

CONFIG.load(&deps.storage).unwrap()
}

/// Helper to create a queue for testing
fn create_test_queue(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>, asset: String) {
use membrane::cdp::QueryMsg as CDP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset};

// Mock CDP basket query
let asset_clone = asset.clone();
deps.querier.update_wasm(move |query| {
    match query {
        cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr == "cdp_contract" {
                let parsed: Result<CDP_QueryMsg, _> = from_json(msg);
                if matches!(parsed, Ok(CDP_QueryMsg::GetBasket {})) {
                    use membrane::types::PendingRevenue;
                    let basket = Basket {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types: vec![cAsset {
                            asset: Asset {
                                info: membrane::types::AssetInfo::NativeToken { 
                                    denom: asset_clone.clone() 
                                },
                                amount: Uint128::zero(),
                            },
                            max_borrow_LTV: Decimal::percent(70),
                            max_LTV: Decimal::percent(75),
                            pool_info: None,
                            rate_index: Decimal::one(),
                            individual_cost: None,
                        }],
                        collateral_supply_caps: vec![],
                        lastest_collateral_rates: vec![],
                        multi_asset_supply_caps: vec![],
                        credit_asset: Asset {
                            info: membrane::types::AssetInfo::NativeToken { denom: "cdt".to_string() },
                            amount: Uint128::zero(),
                        },
                        credit_price: membrane::oracle::PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                            decimals: 6,
                        },
                        base_interest_rate: Decimal::zero(),
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
                        cpc_margin_of_error: Decimal::zero(),
                        liq_queue: None,
                    };
                    return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                        to_json_binary(&basket).unwrap(),
                    ));
                }
            }
            cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unmocked query".to_string(),
                request: msg.clone(),
            })
        }
        _ => cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
            error: "Unmocked query".to_string(),
            request: Default::default(),
        }),
    }
});

let info = mock_info("cdp_contract", &[]);
let env = mock_env();

let msg = ExecuteMsg::CreateQueue { asset };
execute(deps.as_mut(), env, info, msg).unwrap();
}

/// Helper to mock oracle query for price conversion
/// This adds oracle mocking to existing CDP basket mocking
fn mock_oracle_prices(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>, collateral_price: Decimal, cdt_price: Decimal) {
use membrane::cdp::QueryMsg as CDP_QueryMsg;
use membrane::types::{Basket, Asset, cAsset, PendingRevenue};

deps.querier.update_wasm(move |query| {
    match query {
        cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
            // Handle Oracle queries
            if contract_addr == "oracle" {
                let parsed: Result<Oracle_QueryMsg, _> = from_json(msg);
                if let Ok(Oracle_QueryMsg::Prices { .. }) = parsed {
                    let prices = vec![
                        PriceResponse {
                            prices: vec![],
                            price: collateral_price,
                            decimals: 6,
                        },
                        PriceResponse {
                            prices: vec![],
                            price: cdt_price,
                            decimals: 6,
                        },
                    ];
                    return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                        to_json_binary(&prices).unwrap(),
                    ));
                }
            }
            // Handle CDP queries (preserve the mock)
            if contract_addr == "cdp_contract" {
                let parsed: Result<CDP_QueryMsg, _> = from_json(msg);
                if matches!(parsed, Ok(CDP_QueryMsg::GetBasket {})) {
                    let basket = Basket {
                        basket_id: Uint128::one(),
                        current_position_id: Uint128::one(),
                        collateral_types: vec![cAsset {
                            asset: Asset {
                                info: membrane::types::AssetInfo::NativeToken { 
                                    denom: "collateral".to_string() 
                                },
                                amount: Uint128::zero(),
                            },
                            max_borrow_LTV: Decimal::percent(70),
                            max_LTV: Decimal::percent(75),
                            pool_info: None,
                            rate_index: Decimal::one(),
                            individual_cost: None,
                        }],
                        collateral_supply_caps: vec![],
                        lastest_collateral_rates: vec![],
                        multi_asset_supply_caps: vec![],
                        credit_asset: Asset {
                            info: membrane::types::AssetInfo::NativeToken { denom: "cdt".to_string() },
                            amount: Uint128::zero(),
                        },
                        credit_price: membrane::oracle::PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                            decimals: 6,
                        },
                        base_interest_rate: Decimal::zero(),
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
                        cpc_margin_of_error: Decimal::zero(),
                        liq_queue: None,
                    };
                    return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                        to_json_binary(&basket).unwrap(),
                    ));
                }
            }
            cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                error: "Unmocked query".to_string(),
                request: msg.clone(),
            })
        }
        _ => cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
            error: "Unmocked query".to_string(),
            request: Default::default(),
        }),
    }
});
}

#[test]
fn test_bad_debt_fully_covered_by_dispersals() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Setup: Add dispersal with sufficient funds
let dispersal = Dispersal {
    total_to_disperse: Uint128::new(10_000_000), // 10 CDT
    dispersal_window: 24,
    active_dispersal: ActiveDispersal {
        dispersal_start: mock_env().block.time.seconds(),
        amount_dispersed: Uint128::new(0),
    },
    pending_dispersal: Uint128::new(5_000_000), // 5 CDT
};
DISPERSAL.save(&mut deps.storage, "collateral".to_string(), &dispersal).unwrap();

// Execute: Add bad debt of 8 CDT (should be covered by active dispersal)
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(8_000_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Should send 8 CDT to CDP, no deposit slashing
assert_eq!(res.messages.len(), 1);
match &res.messages[0].msg {
    CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
        assert_eq!(contract_addr, "cdp_contract");
        assert_eq!(funds, &vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(8_000_000) }]);
        
        let parsed: CDP_ExecuteMsg = from_json(msg).unwrap();
        assert!(matches!(parsed, CDP_ExecuteMsg::FulfillBadDebt {}));
    }
    _ => panic!("Expected WasmMsg::Execute"),
}

// Verify attributes (method, asset, total_bad_debt_cdt, fulfilled_from_revenue_cdt, slashed_collateral_amount, remaining_bad_debt_cdt)
assert_eq!(res.attributes[2].value, "8000000"); // total_bad_debt_cdt
assert_eq!(res.attributes[3].value, "8000000"); // fulfilled_from_revenue_cdt
assert_eq!(res.attributes[4].value, "0");       // slashed_collateral_amount
assert_eq!(res.attributes[5].value, "0");       // remaining_bad_debt_cdt

// Verify dispersal was updated
let updated_dispersal = DISPERSAL.load(&deps.storage, "collateral".to_string()).unwrap();
assert_eq!(updated_dispersal.total_to_disperse, Uint128::new(2_000_000)); // 10 - 8
assert_eq!(updated_dispersal.pending_dispersal, Uint128::new(5_000_000)); // unchanged
}

#[test]
fn test_bad_debt_uses_active_then_pending_dispersal() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Setup: Active dispersal has 3 CDT remaining, pending has 5 CDT
let dispersal = Dispersal {
    total_to_disperse: Uint128::new(10_000_000),
    dispersal_window: 24,
    active_dispersal: ActiveDispersal {
        dispersal_start: mock_env().block.time.seconds(),
        amount_dispersed: Uint128::new(7_000_000), // 7 already dispersed, 3 remaining
    },
    pending_dispersal: Uint128::new(5_000_000),
};
DISPERSAL.save(&mut deps.storage, "collateral".to_string(), &dispersal).unwrap();

// Execute: Add bad debt of 6 CDT (needs 3 from active + 3 from pending)
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(6_000_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Should fulfill 6 CDT from dispersals
assert_eq!(res.attributes[3].value, "6000000"); // fulfilled_from_revenue_cdt

// Verify dispersal state
let updated_dispersal = DISPERSAL.load(&deps.storage, "collateral".to_string()).unwrap();
assert_eq!(updated_dispersal.total_to_disperse, Uint128::new(7_000_000)); // All remaining was used (10 - 3 taken)
assert_eq!(updated_dispersal.pending_dispersal, Uint128::new(2_000_000)); // 5 - 3
}

#[test]
fn test_bad_debt_requires_deposit_slashing() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle: 1 collateral = $2, 1 CDT = $1
mock_oracle_prices(&mut deps, Decimal::from_ratio(2u128, 1u128), Decimal::one());

// Add deposits at 80% LTV
let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(80),
        max_borrow_ltv: Decimal::percent(75),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// Execute: Add bad debt of 500,000 CDT (no dispersals, must slash deposits)
// 500,000 CDT / $2 per collateral = 250,000 collateral to slash
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(500_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Should have 1 message (swap SubMsg)
assert_eq!(res.messages.len(), 1);

// Check it's a swap SubMsg with reply
let submsg = &res.messages[0];
assert_eq!(submsg.id, LIQUIDATION_SWAP_REPLY_ID);
assert!(matches!(submsg.reply_on, ReplyOn::Success | ReplyOn::Always));

match &submsg.msg {
    CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
        assert_eq!(contract_addr, "chain_proxy");
        assert_eq!(funds.len(), 1);
        assert_eq!(funds[0].denom, "collateral");
        assert_eq!(funds[0].amount, Uint128::new(250_000)); // collateral slashed
        
        let parsed: OsmosisProxy_ExecuteMsg = from_json(msg).unwrap();
        match parsed {
            OsmosisProxy_ExecuteMsg::ExecuteSwaps { token_out, max_slippage } => {
                assert_eq!(token_out, "cdt");
                assert_eq!(max_slippage, Decimal::percent(90));
            }
            _ => panic!("Expected ExecuteSwaps"),
        }
    }
    _ => panic!("Expected WasmMsg::Execute"),
}

// Verify attributes (0:method, 1:asset, 2:total_bad_debt_cdt, 3:fulfilled_from_revenue_cdt, 4:slashed_collateral_amount, 5:remaining_bad_debt_cdt)
assert_eq!(res.attributes[2].value, "500000");  // total_bad_debt_cdt
assert_eq!(res.attributes[3].value, "0");       // fulfilled_from_revenue_cdt
assert_eq!(res.attributes[4].value, "250000");  // slashed_collateral_amount
assert_eq!(res.attributes[5].value, "0");       // remaining_bad_debt_cdt (all handled)

// Verify slot.bad_debt is tracked in CDT terms
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(80)).unwrap();
// bad_debt should be the CDT equivalent of what was slashed: 250,000 collateral * $2 = 500,000 CDT
assert_eq!(slot.bad_debt, Uint128::new(500_000));

// Verify SWAP_PROPAGATION was saved
let swap_prop = SWAP_PROPAGATION.load(&deps.storage).unwrap();
assert!(swap_prop.cdt_balance_before >= Uint128::zero());
}

#[test]
fn test_bad_debt_mixed_revenue_and_slashing() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle: 1 collateral = $1.5, 1 CDT = $1
mock_oracle_prices(&mut deps, Decimal::from_ratio(15u128, 10u128), Decimal::one());

// Setup: 200k CDT in dispersals
let dispersal = Dispersal {
    total_to_disperse: Uint128::new(200_000),
    dispersal_window: 24,
    active_dispersal: ActiveDispersal {
        dispersal_start: mock_env().block.time.seconds(),
        amount_dispersed: Uint128::new(0),
    },
    pending_dispersal: Uint128::new(0),
};
DISPERSAL.save(&mut deps.storage, "collateral".to_string(), &dispersal).unwrap();

// Add deposits
let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(85),
        max_borrow_ltv: Decimal::percent(80),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// Execute: Add bad debt of 500k CDT (200k from dispersals, 300k from slashing)
// 300k CDT / $1.5 per collateral = 200k collateral to slash
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(500_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Should have 2 messages (revenue fulfillment + swap)
assert_eq!(res.messages.len(), 2);

// First message: FulfillBadDebt with 200k CDT from dispersals
match &res.messages[0].msg {
    CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
        assert_eq!(contract_addr, "cdp_contract");
        assert_eq!(funds, &vec![Coin { denom: "cdt".to_string(), amount: Uint128::new(200_000) }]);
        
        let parsed: CDP_ExecuteMsg = from_json(msg).unwrap();
        assert!(matches!(parsed, CDP_ExecuteMsg::FulfillBadDebt {}));
    }
    _ => panic!("Expected WasmMsg::Execute for revenue fulfillment"),
}

// Second message: Swap 200k collateral
match &res.messages[1].msg {
    CosmosMsg::Wasm(WasmMsg::Execute { funds, .. }) => {
        assert_eq!(funds[0].denom, "collateral");
        assert_eq!(funds[0].amount, Uint128::new(200_000));
    }
    _ => panic!("Expected WasmMsg::Execute for swap"),
}

// Verify attributes
// 500k total bad debt - 200k from dispersals = 300k remaining
// 300k CDT / $1.5 per collateral = 200k collateral to slash  
// 200k collateral * $1.5 = 300k CDT value = all remaining covered
assert_eq!(res.attributes[2].value, "500000");  // total_bad_debt_cdt
assert_eq!(res.attributes[3].value, "200000");  // fulfilled_from_revenue_cdt
assert_eq!(res.attributes[4].value, "200000");  // slashed_collateral_amount
assert_eq!(res.attributes[5].value, "0");       // remaining_bad_debt_cdt (all handled)
}

#[test]
fn test_bad_debt_slashes_highest_ltv_first() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle: 1:1 for simplicity
mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());

// Add deposits at different LTV levels
// 75% LTV - 500k collateral (min is 75% from basket)
let deposit_info = mock_info("user1", &coins(500_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(75),
        max_borrow_ltv: Decimal::percent(70),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// 95% LTV - 300k collateral
let deposit_info = mock_info("user2", &coins(300_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(95),
        max_borrow_ltv: Decimal::percent(90),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// 85% LTV - 400k collateral
let deposit_info = mock_info("user3", &coins(400_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(85),
        max_borrow_ltv: Decimal::percent(80),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// Execute: Add bad debt of 600k CDT (should slash from highest LTV first)
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(600_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Correct total slashing from highest LTV slots first
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();

let slashed_collateral: Uint128 = res.attributes.iter()
    .find(|attr| attr.key == "slashed_collateral_amount")
    .and_then(|attr| attr.value.parse::<u128>().ok())
    .map(Uint128::from)
    .unwrap_or(Uint128::zero());

// Total bad debt tracked should equal slashed collateral (1:1 oracle)
let total_bad_debt: Uint128 = queue.slots.iter().map(|s| s.bad_debt).sum();
assert_eq!(total_bad_debt, slashed_collateral, "Total bad debt should match slashed collateral");

// Verify slashing priority: lower LTV should only be slashed after higher LTVs are exhausted
let slot_95 = queue.slots.iter().find(|s| s.ltv == Decimal::percent(95)).unwrap();
let slot_85 = queue.slots.iter().find(|s| s.ltv == Decimal::percent(85)).unwrap();
let slot_75 = queue.slots.iter().find(|s| s.ltv == Decimal::percent(75)).unwrap();
let slot_95_remaining = slot_95.deposit_groups.first()
    .map(|g| g.total_deposit_tokens)
    .unwrap_or(Uint128::zero());
let slot_85_remaining = slot_85.deposit_groups.first()
    .map(|g| g.total_deposit_tokens)
    .unwrap_or(Uint128::zero());
let lowest_slash = slot_75.deposit_groups.first()
    .map(|g| g.total_deposit_tokens)
    .unwrap_or(Uint128::zero());

if lowest_slash < Uint128::new(500_000) {
    // If lowest LTV was slashed, higher LTV slots must be fully slashed
    assert!(slot_95_remaining.is_zero());
    assert!(slot_85_remaining.is_zero());
} else {
    // Otherwise, lowest LTV remains untouched
    assert_eq!(lowest_slash, Uint128::new(500_000));
}
}

#[test]
fn test_bad_debt_early_exit_optimization() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle
mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());

// Add multiple groups at 80% LTV
for i in 0..5 {
    let borrow_ltv = Decimal::percent(70 + i);
    let deposit_info = mock_info(&format!("user{}", i), &coins(100_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: borrow_ltv,
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
}

// Execute: Add bad debt of 250k (should slash 2.5 groups and exit early)
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(250_000),
};

execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Check that slot was processed correctly with early exit
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(80)).unwrap();

// Total slashed should be 250k
assert_eq!(slot.bad_debt, Uint128::new(250_000));

// Verify remaining deposits
let total_remaining: Uint128 = slot.deposit_groups.iter()
    .map(|g| g.total_deposit_tokens)
    .sum();
assert_eq!(total_remaining, Uint128::new(250_000)); // 500k - 250k
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_bad_debt_only_cdp_can_call() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Try to add bad debt from non-CDP address
let info = mock_info("attacker", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(1000),
};

execute(deps.as_mut(), env, info, msg).unwrap();
}

// =============== STRESS TESTS ===============

#[test]
fn stress_test_massive_bad_debt() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle
mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());

// Add 100 deposits across different LTV levels
for i in 0..100 {
    let ltv = Decimal::percent(75 + (i % 20)); // Range from 75-94% (within 75-95% queue range)
    let borrow_ltv = Decimal::percent(70 + (i % 20)); // Range from 70-89%
    let deposit_info = mock_info(&format!("user{}", i), &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv,
            max_borrow_ltv: borrow_ltv,
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
}

// Execute: Add bad debt equal to 50% of all deposits (50M)
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(50_000_000),
};

let res = execute(deps.as_mut(), env, info, msg);
assert!(res.is_ok(), "Massive bad debt should succeed: {:?}", res.err());

// Verify total bad debt tracking
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let total_bad_debt: Uint128 = queue.slots.iter().map(|s| s.bad_debt).sum();
assert_eq!(total_bad_debt, Uint128::new(50_000_000));
}

#[test]
fn stress_test_sequential_bad_debt_events() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle
mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());

// Add initial deposits
for i in 0..10 {
    let deposit_info = mock_info(&format!("user{}", i), &coins(10_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(85),
            max_borrow_ltv: Decimal::percent(80),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
}

// Execute: 20 sequential bad debt events
let info = mock_info("cdp_contract", &[]);
for i in 0..20 {
    let env = mock_env();
    let msg = ExecuteMsg::AddBadDebt {
        asset: "collateral".to_string(),
        amount: Uint128::new(1_000_000),
    };
    
    let res = execute(deps.as_mut(), env, info.clone(), msg);
    assert!(res.is_ok(), "Bad debt event {} should succeed", i);
}

// Verify cumulative bad debt
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(85)).unwrap();
assert_eq!(slot.bad_debt, Uint128::new(20_000_000));
}

#[test]
fn stress_test_extreme_price_ratios() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle: 1 collateral = $100, 1 CDT = $1 (extreme ratio)
mock_oracle_prices(&mut deps, Decimal::from_ratio(100u128, 1u128), Decimal::one());

// Add deposits
let deposit_info = mock_info("user1", &coins(1_000_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(80),
        max_borrow_ltv: Decimal::percent(75),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// Execute: Add bad debt of 50M CDT
// At $100/collateral, this should only slash 500k collateral
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(50_000_000),
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify correct collateral amount was slashed
// 50M CDT / $100 per collateral = 500k collateral
assert_eq!(res.attributes[4].value, "500000"); // slashed_collateral_amount
assert_eq!(res.attributes[5].value, "0");      // remaining_bad_debt_cdt (all handled)

// Verify slot.bad_debt is in CDT terms (50M)
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(80)).unwrap();
assert_eq!(slot.bad_debt, Uint128::new(50_000_000));
}

#[test]
fn stress_test_depleting_all_deposits() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle
mock_oracle_prices(&mut deps, Decimal::one(), Decimal::one());

// Add deposits
let deposit_info = mock_info("user1", &coins(10_000_000, "collateral"));
let deposit_msg = ExecuteMsg::SubmitDeposit {
    deposit_input: BackingDepositInput {
        asset: "collateral".to_string(),
        ltv: Decimal::percent(80),
        max_borrow_ltv: Decimal::percent(75),
        epoch_start_time: Some(0),
    },
    deposit_owner: None,
    locked: None,
    deposit_id: None,
    manager: None,
    affiliate_address: None,
};
execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();

// Execute: Add bad debt exceeding all deposits
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(20_000_000), // More than available
};

let res = execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: Should slash all available deposits and convert to CDT
// 10M collateral * $1 = 10M CDT equivalent slashed
// 20M bad debt - 10M fulfilled = 10M remaining
assert_eq!(res.attributes[4].value, "10000000"); // slashed_collateral_amount (max available)
assert_eq!(res.attributes[5].value, "10000000"); // remaining_bad_debt_cdt (20M - 10M handled)

// Verify all deposits are gone
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(80)).unwrap();
assert_eq!(slot.deposit_groups[0].total_deposit_tokens, Uint128::zero());
assert_eq!(slot.bad_debt, Uint128::new(10_000_000)); // Only what was actually slashed
}

#[test]
fn test_slot_bad_debt_tracking_with_multiple_groups() {
let mut deps = mock_dependencies();
create_test_config(&mut deps);
create_test_queue(&mut deps, "collateral".to_string());

// Mock oracle: 1 collateral = $2, 1 CDT = $1
mock_oracle_prices(&mut deps, Decimal::from_ratio(2u128, 1u128), Decimal::one());

// Add 3 groups at 80% LTV with different borrow LTVs
for i in 0..3 {
    let borrow_ltv = Decimal::percent(70 + i * 2);
    let deposit_info = mock_info(&format!("user{}", i), &coins(1_000_000, "collateral"));
    let deposit_msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "collateral".to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: borrow_ltv,
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), mock_env(), deposit_info, deposit_msg).unwrap();
}

// Execute: Add bad debt that slashes from 2 groups
// 3M CDT bad debt / $2 per collateral = 1.5M collateral to slash
let info = mock_info("cdp_contract", &[]);
let env = mock_env();
let msg = ExecuteMsg::AddBadDebt {
    asset: "collateral".to_string(),
    amount: Uint128::new(3_000_000),
};

execute(deps.as_mut(), env, info, msg).unwrap();

// Verify: slot.bad_debt should equal CDT value of slashed collateral
// 1.5M collateral * $2 = 3M CDT
let queue: LTVQueue = LTV_QUEUES.load(&deps.storage, "collateral".to_string()).unwrap();
let slot = queue.slots.iter().find(|s| s.ltv == Decimal::percent(80)).unwrap();
assert_eq!(slot.bad_debt, Uint128::new(3_000_000)); // Tracked in CDT terms

// Verify deposits were slashed correctly
let total_remaining: Uint128 = slot.deposit_groups.iter()
    .map(|g| g.total_deposit_tokens)
    .sum();
    assert_eq!(total_remaining, Uint128::new(1_500_000)); // 3M - 1.5M slashed
}
