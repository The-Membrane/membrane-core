#![allow(non_snake_case)]
#![allow(dead_code)]

use cosmwasm_std::testing::mock_env;
use cosmwasm_std::{
    Addr, Attribute, Decimal, Event, Reply, SubMsgResponse, SubMsgResult, Timestamp, Uint128,
};

use membrane::liquidation_engine::{Config, LiquidationPropagation};
use membrane::oracle::PriceResponse;
use membrane::types::{cAsset, Asset, AssetInfo};

use crate::state::{CONFIG, LIQUIDATION_PROPAGATION};

// ============================================================================
// Config helpers
// ============================================================================

/// Initialize a default Config: owner=admin, collateral_contract, debt_contract,
/// liq_queue=None, chain_proxy, liq_fee=1%.
pub fn init_config(storage: &mut dyn cosmwasm_std::Storage) -> Config {
    let config = Config {
        owner: Addr::unchecked("admin"),
        collateral_contract: Addr::unchecked("collateral_contract"),
        debt_contract: Addr::unchecked("debt_contract"),
        liq_queue: None,
        chain_proxy: Some(Addr::unchecked("chain_proxy")),
        debt_auction: Some(Addr::unchecked("debt_auction")),
        staking_contract: Some(Addr::unchecked("staking_contract")),
        liq_fee: Decimal::percent(1),
        credit_denom: None,
        liquidation_delay: 28800,
    };
    CONFIG.save(storage, &config).unwrap();
    config
}

/// Initialize Config with a liq_queue configured.
pub fn init_config_with_liq_queue(storage: &mut dyn cosmwasm_std::Storage) -> Config {
    let config = Config {
        owner: Addr::unchecked("admin"),
        collateral_contract: Addr::unchecked("collateral_contract"),
        debt_contract: Addr::unchecked("debt_contract"),
        liq_queue: Some(Addr::unchecked("liq_queue")),
        chain_proxy: Some(Addr::unchecked("chain_proxy")),
        debt_auction: Some(Addr::unchecked("debt_auction")),
        staking_contract: Some(Addr::unchecked("staking_contract")),
        liq_fee: Decimal::percent(1),
        credit_denom: None,
        liquidation_delay: 28800,
    };
    CONFIG.save(storage, &config).unwrap();
    config
}

/// Initialize Config with custom liq_queue and liq_fee.
pub fn init_config_custom(
    storage: &mut dyn cosmwasm_std::Storage,
    liq_queue: Option<&str>,
    liq_fee: Decimal,
) -> Config {
    let config = Config {
        owner: Addr::unchecked("admin"),
        collateral_contract: Addr::unchecked("collateral_contract"),
        debt_contract: Addr::unchecked("debt_contract"),
        liq_queue: liq_queue.map(Addr::unchecked),
        chain_proxy: Some(Addr::unchecked("chain_proxy")),
        debt_auction: Some(Addr::unchecked("debt_auction")),
        staking_contract: Some(Addr::unchecked("staking_contract")),
        liq_fee,
        credit_denom: None,
        liquidation_delay: 28800,
    };
    CONFIG.save(storage, &config).unwrap();
    config
}

/// Initialize Config with custom liquidation delay.
pub fn init_config_with_delay(
    storage: &mut dyn cosmwasm_std::Storage,
    liquidation_delay: u64,
) -> Config {
    let config = Config {
        owner: Addr::unchecked("admin"),
        collateral_contract: Addr::unchecked("collateral_contract"),
        debt_contract: Addr::unchecked("debt_contract"),
        liq_queue: None,
        chain_proxy: Some(Addr::unchecked("chain_proxy")),
        debt_auction: Some(Addr::unchecked("debt_auction")),
        staking_contract: Some(Addr::unchecked("staking_contract")),
        liq_fee: Decimal::percent(1),
        credit_denom: None,
        liquidation_delay,
    };
    CONFIG.save(storage, &config).unwrap();
    config
}

// ============================================================================
// Propagation helpers
// ============================================================================

/// Create a zeroed LiquidationPropagation with given owner/id.
pub fn default_propagation() -> LiquidationPropagation {
    LiquidationPropagation {
        per_asset_repayment: vec![],
        liq_queue_repayment: Decimal::zero(),
        protocol_fee_value: Decimal::zero(),
        caller_fee_assets: vec![],
        protocol_fee_assets: vec![],
        sell_wall_amounts: vec![],
        cAsset_ratios: vec![],
        cAsset_prices: vec![],
        collateral_assets: vec![],
        total_debt: Uint128::zero(),
        credit_price: make_price(Decimal::zero(), 6),
        caller_fee_value_paid: Decimal::zero(),
        total_repaid: Decimal::zero(),
        venue_recall_amount: Uint128::zero(),
        position_owner: Addr::unchecked("position_owner"),
        position_id: Uint128::one(),
        caller: Addr::unchecked("liquidator"),
        timer_start_only: false,
        liquidation_outcome: None,
        timer_started_at: None,
        current_ltv: Decimal::zero(),
    }
}

/// Save propagation to storage.
pub fn save_propagation(
    storage: &mut dyn cosmwasm_std::Storage,
    prop: &LiquidationPropagation,
) {
    LIQUIDATION_PROPAGATION.save(storage, prop).unwrap();
}

/// Make a propagation in post-debt-accrue state (Step 1 completed).
/// Has total_debt and credit_price set, everything else zeroed.
pub fn make_propagation_post_debt_accrue(
    total_debt: u128,
    credit_price: Decimal,
    decimals: u64,
) -> LiquidationPropagation {
    LiquidationPropagation {
        total_debt: Uint128::new(total_debt),
        credit_price: make_price(credit_price, decimals),
        ..default_propagation()
    }
}

/// Make a propagation in post-assess state (Step 2 completed).
/// Has collateral_assets, ratios, prices, fees, per_asset_repayment all set.
pub fn make_propagation_post_assess(
    total_debt: u128,
    credit_price: Decimal,
    credit_decimals: u64,
    collateral_assets: Vec<cAsset>,
    ratios: Vec<Decimal>,
    prices: Vec<PriceResponse>,
    per_asset_repayment: Vec<Decimal>,
    caller_fee: Decimal,
    protocol_fee: Decimal,
    total_repaid: Decimal,
) -> LiquidationPropagation {
    LiquidationPropagation {
        per_asset_repayment,
        liq_queue_repayment: Decimal::zero(),
        protocol_fee_value: protocol_fee,
        caller_fee_assets: vec![],
        protocol_fee_assets: vec![],
        sell_wall_amounts: vec![],
        cAsset_ratios: ratios,
        cAsset_prices: prices,
        collateral_assets,
        total_debt: Uint128::new(total_debt),
        credit_price: make_price(credit_price, credit_decimals),
        caller_fee_value_paid: caller_fee,
        total_repaid,
        venue_recall_amount: Uint128::zero(),
        position_owner: Addr::unchecked("position_owner"),
        position_id: Uint128::one(),
        caller: Addr::unchecked("liquidator"),
        timer_start_only: false,
        liquidation_outcome: None,
        timer_started_at: None,
        current_ltv: Decimal::zero(),
    }
}

// ============================================================================
// Price / Asset helpers
// ============================================================================

/// Make a PriceResponse with given price and decimals.
pub fn make_price(price: Decimal, decimals: u64) -> PriceResponse {
    PriceResponse {
        prices: vec![],
        price,
        decimals,
    }
}

/// Make a cAsset with given denom, amount, borrow LTV %, and max LTV %.
pub fn make_casset(denom: &str, amount: u128, borrow_ltv_pct: u64, max_ltv_pct: u64) -> cAsset {
    cAsset {
        asset: Asset {
            info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            amount: Uint128::new(amount),
        },
        max_borrow_LTV: Decimal::percent(borrow_ltv_pct),
        max_LTV: Decimal::percent(max_ltv_pct),
        rate_index: Decimal::one(),
        peg_rate_index: Decimal::one(),
        pool_info: None,
        force_redemptions: None,
        max_threshold_to_delay: Decimal::percent(15),
    }
}

/// Make a cAsset with custom max_threshold_to_delay.
pub fn make_casset_with_threshold(denom: &str, amount: u128, borrow_ltv_pct: u64, max_ltv_pct: u64, threshold_pct: u64) -> cAsset {
    cAsset {
        asset: Asset {
            info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            amount: Uint128::new(amount),
        },
        max_borrow_LTV: Decimal::percent(borrow_ltv_pct),
        max_LTV: Decimal::percent(max_ltv_pct),
        rate_index: Decimal::one(),
        peg_rate_index: Decimal::one(),
        pool_info: None,
        force_redemptions: None,
        max_threshold_to_delay: Decimal::percent(threshold_pct),
    }
}

/// Make a simple Asset.
pub fn make_asset(denom: &str, amount: u128) -> Asset {
    Asset {
        info: AssetInfo::NativeToken {
            denom: denom.to_string(),
        },
        amount: Uint128::new(amount),
    }
}

// ============================================================================
// Reply builders
// ============================================================================

/// Build a success Reply with given id and attributes.
pub fn make_success_reply(id: u64, attrs: Vec<(&str, &str)>) -> Reply {
    let attributes: Vec<Attribute> = attrs
        .into_iter()
        .map(|(k, v)| Attribute {
            key: k.to_string(),
            value: v.to_string(),
        })
        .collect();

    Reply {
        id,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attributes(attributes)],
            data: None,
            msg_responses: vec![],
        }),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    }
}

/// Build an error Reply with given id and error string.
pub fn make_error_reply(id: u64, error: &str) -> Reply {
    Reply {
        id,
        result: SubMsgResult::Err(error.to_string()),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    }
}

/// Build a success Reply for the debt accrue step.
/// Serializes AccrueDebtResponse as JSON string in the "debt_response" attribute.
pub fn make_debt_accrue_success_reply(
    total_debt: u128,
    credit_price: Decimal,
    decimals: u64,
) -> Reply {
    use membrane::debt::AccrueDebtResponse;

    let response = AccrueDebtResponse {
        total_debt: Uint128::new(total_debt),
        credit_price: make_price(credit_price, decimals),
        rate_segments: vec![],
        peg_rate_segments: vec![],
        venue_recall_amount: Uint128::zero(),
    };
    let json = cosmwasm_std::to_json_string(&response).unwrap();

    make_success_reply(
        crate::state::DEBT_ACCRUE_REPLY_ID,
        vec![("debt_response", &json)],
    )
}

/// Build a success Reply for the collateral assess step.
/// Serializes CollateralAssessmentResponse as JSON string in the "assessment" attribute.
pub fn make_collateral_assess_success_reply(
    collateral_assets: Vec<cAsset>,
    ratios: Vec<Decimal>,
    prices: Vec<PriceResponse>,
    avg_max_ltv: Decimal,
    avg_max_borrow_ltv: Decimal,
    any_frozen: bool,
    frozen_assets: Vec<String>,
) -> Reply {
    make_collateral_assess_success_reply_with_threshold(
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        Decimal::zero(), // Default: no delay zone (matches pre-per-collateral behavior)
        any_frozen,
        frozen_assets,
    )
}

/// Build a success Reply for the collateral assess step with custom threshold.
pub fn make_collateral_assess_success_reply_with_threshold(
    collateral_assets: Vec<cAsset>,
    ratios: Vec<Decimal>,
    prices: Vec<PriceResponse>,
    avg_max_ltv: Decimal,
    avg_max_borrow_ltv: Decimal,
    avg_max_threshold_to_delay: Decimal,
    any_frozen: bool,
    frozen_assets: Vec<String>,
) -> Reply {
    use membrane::collateral::CollateralAssessmentResponse;

    let assessment = CollateralAssessmentResponse {
        collateral_assets,
        ratios,
        prices,
        avg_max_ltv,
        avg_max_borrow_ltv,
        avg_max_threshold_to_delay,
        any_frozen,
        frozen_assets,
    };
    let json = cosmwasm_std::to_json_string(&assessment).unwrap();

    make_success_reply(
        crate::state::COLLATERAL_ASSESS_REPLY_ID,
        vec![("assessment", &json)],
    )
}

/// Build a success Reply for the liq queue step.
/// Attributes: repay_amount, collateral_amount, collateral_token.
pub fn make_liq_queue_success_reply(
    repay_amount: u128,
    collateral_amount: u128,
    collateral_token: &str,
) -> Reply {
    let repay_str = repay_amount.to_string();
    let coll_str = collateral_amount.to_string();
    make_success_reply(
        crate::state::LIQ_QUEUE_REPLY_ID,
        vec![
            ("repay_amount", &repay_str),
            ("collateral_amount", &coll_str),
            ("collateral_token", collateral_token),
        ],
    )
}

// ============================================================================
// Env helpers
// ============================================================================

/// Make a mock env at a specific timestamp.
pub fn mock_env_at(time_secs: u64) -> cosmwasm_std::Env {
    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(time_secs);
    env
}
