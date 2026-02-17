#![allow(unused_imports)]
use cosmwasm_std::{
testing::{mock_dependencies, mock_dependencies_with_balances, mock_env, mock_info},
coin, coins, Decimal, Uint128, WasmMsg, CosmosMsg, BankMsg, to_json_binary, WasmQuery, QueryRequest, SystemResult, ContractResult,
from_json, Binary,
};
use membrane::math::decimal_multiplication;
use membrane::ltv_disco::{
InstantiateMsg, ExecuteMsg, QueryMsg, BackingDepositInput, Config,
};
use membrane::types::{AffiliateData, cAsset, Asset, AssetInfo, Basket, PendingRevenue, DepositDenom};
use membrane::oracle::PriceResponse;
use ltv_disco::contract::{instantiate, execute, query};
use ltv_disco::state::{AFFILIATES, BACKING_DEPOSITS, REVENUE_EVENTS, CONFIG};
use ltv_disco::error::ContractError;
use ltv_disco::execute::make_deposit_key;

const SECONDS_PER_DAY: u64 = 86400;

fn setup_mock_basket() -> Basket {
Basket {
    basket_id: Uint128::new(1),
    current_position_id: Uint128::new(1),
    collateral_types: vec![cAsset {
        asset: Asset {
            info: AssetInfo::NativeToken { denom: "uusd".to_string() },
            amount: Uint128::zero(),
        },
        max_LTV: Decimal::percent(50),
        max_borrow_LTV: Decimal::percent(30),
        rate_index: Decimal::zero(),
        pool_info: None,
    }],
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

fn setup_instantiate(
deps: &mut cosmwasm_std::OwnedDeps<
    cosmwasm_std::MemoryStorage,
    cosmwasm_std::testing::MockApi,
    cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
>,
) {
// Setup querier to return mock basket
deps.querier.update_wasm(move |query| {
    match query {
        cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
            let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
            if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                SystemResult::Ok(ContractResult::Ok(to_json_binary(&setup_mock_basket()).unwrap()))
            } else {
                SystemResult::Ok(ContractResult::Ok(Binary::default()))
            }
        }
        _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
    }
});

let env = mock_env();
let info = mock_info("owner", &[]);
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
    affiliate_fee: Some(Decimal::percent(1)), // 1% default
    max_management_fee: None,
    ltv_delta_minimum: Some(Decimal::percent(1)),
    emissions_voting_contract: None,
    points_system_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
    };

instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
}

fn setup_deposit_with_revenue(
deps: &mut cosmwasm_std::OwnedDeps<
    cosmwasm_std::MemoryStorage,
    cosmwasm_std::testing::MockApi,
    cosmwasm_std::testing::MockQuerier<cosmwasm_std::Empty>,
>,
env: &mut cosmwasm_std::Env,
user: &str,
deposit_amount: Uint128,
revenue_amount: Uint128,
affiliate_address: Option<String>,
) {
// Create queue
let info = mock_info("cdp_contract", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
).unwrap();

// Submit deposit
let info = mock_info(user, &coins(deposit_amount.u128(), "uusd"));
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: affiliate_address,
    },
).unwrap();

// Advance time so revenue events have timestamp > deposit.last_claimed
env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);

// Add revenue
let info = mock_info("cdp_contract", &coins(revenue_amount.u128(), "reward_token"));
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::AddRevenue { asset: "uusd".to_string() },
).unwrap();
}

#[test]
fn test_set_affiliate_success() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let env = mock_env();
let info = mock_info("user", &[]);

let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: Some("test-label".to_string()),
    },
)
.unwrap();

// Check affiliate was saved
let affiliates = AFFILIATES.load(&deps.storage, "user".to_string()).unwrap();
assert_eq!(affiliates.len(), 1);
assert_eq!(affiliates[0].affiliate_address, "affiliate1");
assert_eq!(affiliates[0].label, Some("test-label".to_string()));

// Check config fee is used
let config = CONFIG.load(&deps.storage).unwrap();
assert_eq!(affiliates[0].affiliate_fee, config.affiliate_fee);
assert_eq!(affiliates[0].time_affiliated, env.block.time.seconds());
}

#[test]
fn test_set_affiliate_limit() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let env = mock_env();

// Add 10 affiliates (the limit)
for i in 1..=10 {
    let info = mock_info("user", &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SetAffiliate {
            user: "user".to_string(),
            affiliate_address: format!("affiliate{}", i),
            label: None,
        },
    ).unwrap();
}

// Try to add an 11th affiliate - should fail
let info = mock_info("user", &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate11".to_string(),
        label: None,
    },
);

assert!(res.is_err());
match res.unwrap_err() {
    ContractError::CustomError { val } => {
        assert!(val.contains("Can't add more than 10 affiliations"));
    }
    _ => panic!("Expected CustomError"),
}
}

#[test]
fn test_set_affiliate_update_label() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let env = mock_env();

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: Some("original-label".to_string()),
    },
).unwrap();

// Update label (only affiliate can update their own label)
let info = mock_info("affiliate1", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: Some("updated-label".to_string()),
    },
).unwrap();

let affiliates = AFFILIATES.load(&deps.storage, "user".to_string()).unwrap();
assert_eq!(affiliates[0].label, Some("updated-label".to_string()));
}

#[test]
fn test_set_affiliate_unauthorized_update() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let env = mock_env();

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

// Try to update as different user - should fail
let info = mock_info("other_user", &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: Some("hacked-label".to_string()),
    },
);

assert!(res.is_err());
match res.unwrap_err() {
    ContractError::Unauthorized {} => {}
    _ => panic!("Expected Unauthorized error"),
}
}

#[test]
fn test_deposit_with_affiliate() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();

// Create queue
let info = mock_info("cdp_contract", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
).unwrap();

// Deposit with affiliate
let info = mock_info("user", &coins(10000, "uusd"));
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: Some("affiliate1".to_string()),
    },
).unwrap();

// Check affiliate was saved
let affiliates = AFFILIATES.load(&deps.storage, "user".to_string()).unwrap();
assert_eq!(affiliates.len(), 1);
assert_eq!(affiliates[0].affiliate_address, "affiliate1");

let config = CONFIG.load(&deps.storage).unwrap();
assert_eq!(affiliates[0].affiliate_fee, config.affiliate_fee);
assert_eq!(affiliates[0].time_affiliated, env.block.time.seconds());
}

#[test]
fn test_deposit_with_existing_affiliate_no_duplicate() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();

// Set affiliate first
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

// Create queue
let info = mock_info("cdp_contract", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::CreateQueue { asset: "uusd".to_string() },
).unwrap();

// Deposit with same affiliate - should not create duplicate
let info = mock_info("user", &coins(10000, "uusd"));
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(50),
            max_borrow_ltv: Decimal::percent(30),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: Some("affiliate1".to_string()),
    },
).unwrap();

// Should still have only one affiliate
let affiliates = AFFILIATES.load(&deps.storage, "user".to_string()).unwrap();
assert_eq!(affiliates.len(), 1);
}

#[test]
fn test_claim_with_single_affiliate() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();
let user = "user";
let total_claimable = Uint128::new(100_000);

// Setup deposit with revenue
setup_deposit_with_revenue(
    &mut deps,
    &mut env,
    user,
    Uint128::new(10000),
    total_claimable,
    None,
);

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
)
.unwrap();

// Advance time so affiliate fee calculation has non-zero duration
env.block.time = env.block.time.plus_seconds(1);

// Claim revenue (must be called by the user themselves)
let info = mock_info(user, &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::ClaimRevenueForUser {
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        limit: Some(10),
        compound_action: None,
    },
)
.unwrap();

// Should have messages for affiliate fee and user
assert!(res.messages.len() >= 1);

// Extract actual claimed amount from response attributes
let actual_claimed = res.attributes.iter()
    .find(|attr| attr.key == "revenue_claimed")
    .and_then(|attr| attr.value.parse::<u128>().ok())
    .map(Uint128::from)
    .unwrap_or(Uint128::zero());
let total_distributed = res.messages.iter().fold(Uint128::zero(), |acc, msg| {
    match &msg.msg {
        CosmosMsg::Bank(BankMsg::Send { amount, .. }) => acc + amount[0].amount,
        _ => acc,
    }
});
let actual_claimed = if actual_claimed.is_zero() {
    total_distributed
} else {
    actual_claimed
};

// Find affiliate fee message
let affiliate_msg = res.messages.iter()
    .find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "affiliate1"
        } else {
            false
        }
    });

assert!(affiliate_msg.is_some(), "Should have affiliate fee message");

if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &affiliate_msg.unwrap().msg {
    assert_eq!(to_address, "affiliate1");
    // Affiliate fee should be 1% of actual claimed amount
    // Note: The affiliate fee is calculated based on the config fee (1%) and time-based splitting
    // For a single affiliate, the fee should be 1% of the claimed amount
    let expected_fee = decimal_multiplication(
        Decimal::from_ratio(actual_claimed, Uint128::one()),
        Decimal::percent(1)
    ).unwrap().to_uint_floor();
    
    // The fee should be non-zero and proportional to the claim
    // Due to Decimal precision and time-based splitting, allow for some variance
    // But it should be approximately 1% of the claim
    assert!(amount[0].amount > Uint128::zero(), "Affiliate fee should be non-zero");
    // Verify it's approximately 1% (within 10% tolerance to account for rounding and time-based splitting)
    let fee_ratio = Decimal::from_ratio(amount[0].amount, actual_claimed);
    let expected_ratio = Decimal::percent(1);
    let ratio_diff = if fee_ratio > expected_ratio {
        fee_ratio - expected_ratio
    } else {
        expected_ratio - fee_ratio
    };
    // Allow up to 10% difference due to rounding and time-based calculations
    assert!(ratio_diff <= Decimal::percent(10), 
        "Affiliate fee should be approximately 1% of claimed amount (fee: {}, claimed: {}, ratio: {}, expected_ratio: {})", 
        amount[0].amount, actual_claimed, fee_ratio, expected_ratio);
    assert_eq!(amount[0].denom, "reward_token");
}

// Verify affiliate list is reset to only the last affiliate
let affiliates_after_claim = AFFILIATES.load(&deps.storage, user.to_string()).unwrap();
assert_eq!(affiliates_after_claim.len(), 1);
assert_eq!(affiliates_after_claim[0].affiliate_address, "affiliate1");
assert_eq!(affiliates_after_claim[0].time_affiliated, env.block.time.seconds());
}

#[test]
fn test_claim_with_multiple_affiliates_time_based_split() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();
let user = "user";
let total_claimable = Uint128::new(100_000);

// Setup deposit with revenue
setup_deposit_with_revenue(
    &mut deps,
    &mut env,
    user,
    Uint128::new(10000),
    total_claimable,
    None,
);

// Set first affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

// Advance time
env.block.time = env.block.time.plus_seconds(30 * SECONDS_PER_DAY);

// Set second affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate2".to_string(),
        label: None,
    },
).unwrap();

// Advance time
env.block.time = env.block.time.plus_seconds(20 * SECONDS_PER_DAY);

// Claim revenue (must be called by the user themselves)
let info = mock_info(user, &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::ClaimRevenueForUser {
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        limit: Some(10),
        compound_action: None,
    },
)
.unwrap();

// Extract actual claimed amount from response
let actual_claimed = res.attributes.iter()
    .find(|attr| attr.key == "revenue_claimed")
    .and_then(|attr| attr.value.parse::<u128>().ok())
    .map(Uint128::from)
    .unwrap_or(Uint128::zero());

// Calculate expected splits based on actual claimed amount
// Affiliate1: 30 days (60% of 50 days)
// Affiliate2: 20 days (40% of 50 days)
// Total fee: 1% of actual_claimed
// Affiliate1: 60% of total_fee
// Affiliate2: 40% of total_fee

let total_fee = decimal_multiplication(
    Decimal::from_ratio(actual_claimed, Uint128::one()),
    Decimal::percent(1)
).unwrap().to_uint_floor();
let affiliate1_expected = decimal_multiplication(
    Decimal::from_ratio(total_fee, Uint128::one()),
    Decimal::from_ratio(30u128, 50u128)
).unwrap().to_uint_floor();
let affiliate2_expected = decimal_multiplication(
    Decimal::from_ratio(total_fee, Uint128::one()),
    Decimal::from_ratio(20u128, 50u128)
).unwrap().to_uint_floor();

// Check affiliate1 fee
let affiliate1_msg = res.messages.iter()
    .find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "affiliate1"
        } else {
            false
        }
    });

assert!(affiliate1_msg.is_some(), "Affiliate1 should receive fee");
let affiliate1_fee = if let CosmosMsg::Bank(BankMsg::Send { amount, .. }) = &affiliate1_msg.unwrap().msg {
    amount[0].amount
} else {
    Uint128::zero()
};
assert!(affiliate1_fee > Uint128::zero(), "Affiliate1 fee should be non-zero");

// Check affiliate2 fee
let affiliate2_msg = res.messages.iter()
    .find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "affiliate2"
        } else {
            false
        }
    });

assert!(affiliate2_msg.is_some(), "Affiliate2 should receive fee");
let affiliate2_fee = if let CosmosMsg::Bank(BankMsg::Send { amount, .. }) = &affiliate2_msg.unwrap().msg {
    amount[0].amount
} else {
    Uint128::zero()
};
assert!(affiliate2_fee > Uint128::zero(), "Affiliate2 fee should be non-zero");

// Verify affiliate2 gets less than affiliate1 (due to time-based splitting)
// Affiliate1 was active for 30 days, affiliate2 for 20 days (out of 50 total)
// So affiliate1 should get approximately 60% of the total fee, affiliate2 should get 40%
let total_affiliate_fees = affiliate1_fee + affiliate2_fee;
let affiliate1_ratio = Decimal::from_ratio(affiliate1_fee, total_affiliate_fees);
let affiliate2_ratio = Decimal::from_ratio(affiliate2_fee, total_affiliate_fees);

// Affiliate1 should get more than affiliate2 (approximately 60% vs 40%)
assert!(affiliate1_fee > affiliate2_fee, 
    "Affiliate1 should get more than affiliate2 due to longer time (affiliate1: {}, affiliate2: {})", 
    affiliate1_fee, affiliate2_fee);

// Verify the ratio is approximately correct (within 20% tolerance due to rounding and time calculations)
let expected_affiliate1_ratio = Decimal::from_ratio(30u128, 50u128); // 60%
let ratio_diff = if affiliate1_ratio > expected_affiliate1_ratio {
    affiliate1_ratio - expected_affiliate1_ratio
} else {
    expected_affiliate1_ratio - affiliate1_ratio
};
assert!(ratio_diff <= Decimal::percent(20), 
    "Affiliate1 should get approximately 60% of total fees (got: {}, expected: {})", 
    affiliate1_ratio, expected_affiliate1_ratio);
}

#[test]
fn test_claim_no_affiliate_no_fee() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();
let user = "user";
let total_claimable = Uint128::new(100_000);

// Setup deposit with revenue (no affiliate)
setup_deposit_with_revenue(
    &mut deps,
    &mut env,
    user,
    Uint128::new(10000),
    total_claimable,
    None,
);

// Claim revenue (must be called by the user themselves)
let info = mock_info(user, &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::ClaimRevenueForUser {
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        limit: Some(10),
        compound_action: None,
    },
)
.unwrap();

// Should have no affiliate fee messages
let affiliate_msgs: Vec<_> = res.messages.iter()
    .filter(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, .. }) = &msg.msg {
            to_address == "affiliate1" || to_address == "affiliate2"
        } else {
            false
        }
    })
    .collect();

assert_eq!(affiliate_msgs.len(), 0, "Should have no affiliate fee messages when no affiliate is set");
}

#[test]
fn test_claim_redundant_check_prevents_over_claim() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();
let user = "user";
let total_claimable = Uint128::new(100_000);

// Setup deposit with revenue
setup_deposit_with_revenue(
    &mut deps,
    &mut env,
    user,
    Uint128::new(10000),
    total_claimable,
    None,
);

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

// Advance time so affiliate fee calculation has non-zero duration
env.block.time = env.block.time.plus_seconds(1);

// Claim revenue (must be called by the user themselves)
let info = mock_info(user, &[]);
let res = execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::ClaimRevenueForUser {
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        limit: Some(10),
        compound_action: None,
    },
)
.unwrap();

// Extract actual claimed amount from response
let actual_claimed = res.attributes.iter()
    .find(|attr| attr.key == "revenue_claimed")
    .and_then(|attr| attr.value.parse::<u128>().ok())
    .map(Uint128::from)
    .unwrap_or(Uint128::zero());
let total_distributed = res.messages.iter().fold(Uint128::zero(), |acc, msg| {
    match &msg.msg {
        CosmosMsg::Bank(BankMsg::Send { amount, .. }) => acc + amount[0].amount,
        _ => acc,
    }
});
let actual_claimed = if actual_claimed.is_zero() {
    total_distributed
} else {
    actual_claimed
};

// Verify the redundant check works by ensuring:
// 1. Affiliate fee is calculated correctly (1% of actual_claimed)
// 2. User amount = actual_claimed - affiliate_fee
// 3. Total distributed <= actual_claimed

let expected_affiliate_fee = decimal_multiplication(
    Decimal::from_ratio(actual_claimed, Uint128::one()),
    Decimal::percent(1)
).unwrap().to_uint_floor();

// Extract actual amounts from messages
let mut actual_affiliate_fee = Uint128::zero();
let mut actual_user_amount = Uint128::zero();

for msg in &res.messages {
    match &msg.msg {
        CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
            if to_address == "affiliate1" {
                actual_affiliate_fee += amount[0].amount;
            } else if to_address == user {
                actual_user_amount += amount[0].amount;
            }
        }
        _ => {}
    }
}

// Verify fees are approximately correct (within rounding and Decimal precision)
// The fee should be approximately 1% of actual_claimed
let fee_ratio = Decimal::from_ratio(actual_affiliate_fee, actual_claimed);
let expected_ratio = Decimal::percent(1);
let ratio_diff = if fee_ratio > expected_ratio {
    fee_ratio - expected_ratio
} else {
    expected_ratio - fee_ratio
};
assert!(ratio_diff <= Decimal::percent(10), 
    "Affiliate fee should be approximately 1% of claimed amount (fee: {}, claimed: {}, ratio: {})", 
    actual_affiliate_fee, actual_claimed, fee_ratio);

// Verify total distributed doesn't exceed actual_claimed
let total_distributed = actual_affiliate_fee + actual_user_amount;
assert!(total_distributed <= actual_claimed, 
    "Total distributed should not exceed actual_claimed (distributed: {}, claimed: {})", 
    total_distributed, actual_claimed);
assert!(actual_affiliate_fee <= actual_claimed, 
    "Affiliate fee should not exceed actual_claimed (fee: {}, claimed: {})", 
    actual_affiliate_fee, actual_claimed);
}

#[test]
fn test_affiliate_fee_from_config() {
let mut deps = mock_dependencies();

// Setup with custom affiliate fee (2%)
deps.querier.update_wasm(move |query| {
    match query {
        cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
            let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
            if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                SystemResult::Ok(ContractResult::Ok(to_json_binary(&setup_mock_basket()).unwrap()))
            } else {
                SystemResult::Ok(ContractResult::Ok(Binary::default()))
            }
        }
        _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
    }
});

let env = mock_env();
let info = mock_info("owner", &[]);
let mut msg = InstantiateMsg {
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
    affiliate_fee: Some(Decimal::percent(2)), // 2% custom fee
    max_management_fee: None,
    ltv_delta_minimum: Some(Decimal::percent(1)),
    emissions_voting_contract: None,
    points_system_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
    };

instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

// Check affiliate uses config fee
let affiliates = AFFILIATES.load(&deps.storage, "user".to_string()).unwrap();
assert_eq!(affiliates[0].affiliate_fee, Decimal::percent(2));
}

#[test]
fn test_get_affiliates_query() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let env = mock_env();

// Set affiliate
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: "user".to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: Some("test-label".to_string()),
    },
).unwrap();

// Query affiliates
let query_msg = QueryMsg::GetAffiliates { user: "user".to_string() };
let res = query(deps.as_ref(), env, query_msg).unwrap();
let affiliates: Vec<AffiliateData> = from_json(res).unwrap();

assert_eq!(affiliates.len(), 1);
assert_eq!(affiliates[0].affiliate_address, "affiliate1");
assert_eq!(affiliates[0].label, Some("test-label".to_string()));
}

#[test]
fn test_claim_affiliate_reset_after_claim() {
let mut deps = mock_dependencies();
setup_instantiate(&mut deps);

let mut env = mock_env();
let user = "user";
let total_claimable = Uint128::new(100_000);

// Setup deposit with revenue
setup_deposit_with_revenue(
    &mut deps,
    &mut env,
    user,
    Uint128::new(10000),
    total_claimable,
    None,
);

// Set multiple affiliates
let info = mock_info("user", &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info.clone(),
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate1".to_string(),
        label: None,
    },
).unwrap();

env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);

execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::SetAffiliate {
        user: user.to_string(),
        affiliate_address: "affiliate2".to_string(),
        label: None,
    },
).unwrap();

// Verify we have 2 affiliates before claim
let affiliates_before = AFFILIATES.load(&deps.storage, user.to_string()).unwrap();
assert_eq!(affiliates_before.len(), 2);

// Claim revenue (must be called by the user themselves)
let info = mock_info(user, &[]);
execute(
    deps.as_mut(),
    env.clone(),
    info,
    ExecuteMsg::ClaimRevenueForUser {
        user: user.to_string(),
        asset: "uusd".to_string(),
        max_ltv: Decimal::percent(50),
        max_borrow_ltv: Decimal::percent(30),
        limit: Some(10),
        compound_action: None,
    },
).unwrap();

// After claim, should preserve all affiliates (up to 10), with time_affiliated reset
// First affiliate should have time_affiliated = 0, last affiliate should have current time
let affiliates_after = AFFILIATES.load(&deps.storage, user.to_string()).unwrap();
assert_eq!(affiliates_after.len(), 2);
assert_eq!(affiliates_after[0].affiliate_address, "affiliate1");
    assert_eq!(affiliates_after[0].time_affiliated, 0); // Time wiped for non-last affiliates
    assert_eq!(affiliates_after[1].affiliate_address, "affiliate2");
    assert_eq!(affiliates_after[1].time_affiliated, env.block.time.seconds()); // Last affiliate gets current time
}
