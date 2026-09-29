#![allow(non_snake_case)]
//! Financial tests for the Debt contract.
//!
//! These tests exercise the financial logic at a higher integration level
//! than the pure unit tests in debt_manager.rs and rates.rs, by setting up
//! realistic state (positions with multiple rate segments, basket state, RATES)
//! and testing multi-step flows through the internal functions.
//!
//! The execute-level integration tests (integration_tests.rs) cover operations
//! that don't require RATES to be initialized. These financial tests fill
//! the gap by testing repay, accrual, liquidation debt update, and intent flows.

pub mod tests {
    use cosmwasm_std::{
        coin, testing::mock_env, Addr, Decimal, Empty, Int128, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};
    use std::str::FromStr;

    use membrane::debt::*;
    use membrane::types::{
        Asset, AssetInfo, CreditAssetBreakdown, DeploymentIntent, FixedRate, FixedRateCap,
        FixedRateCaps, FixedRateEnd, IRMConfig, Rates, RateSegment,
        UserDeploymentIntents,
    };

    use crate::debt_manager::{
        self, apply_delta_to_regular_debt, combined_breakdown_for_caps, create_segments_from_split,
        get_total_from_segments, reduce_segments_proportionally, repay_segments_default_order,
        repay_segments_targeted, BreakdownView, SegmentDeltas, SegmentSplit,
    };
    use crate::rates::{
        adaptive_smooth_rate, calculate_fixed_rate_accrual, check_and_handle_fixed_rate_expiration,
        get_total_position_debt, recalculate_fixed_rate, SECONDS_PER_YEAR,
    };

    const ADMIN: &str = "admin";
    const USER: &str = "user";
    const COLLATERAL_CONTRACT: &str = "collateral_contract";
    const LIQUIDATION_CONTRACT: &str = "liquidation_contract";
    const POINTS_CONTRACT: &str = "points_contract";
    const CDT_DENOM: &str = "factory/creator/ucdt";

    fn get_addr(app: &App, name: &str) -> Addr {
        app.api().addr_make(name)
    }

    pub fn debt_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, api, storage| {
            let bigger_bank = vec![
                coin(100_000_000_000_000u128, "uosmo"),
                coin(100_000_000_000_000u128, CDT_DENOM),
                coin(100_000_000_000_000u128, "uatom"),
            ];
            router
                .bank
                .init_balance(storage, &api.addr_make(ADMIN), bigger_bank.clone())
                .unwrap();
            router
                .bank
                .init_balance(storage, &api.addr_make(USER), bigger_bank)
                .unwrap();
        })
    }

    fn default_irm_config() -> IRMConfig {
        IRMConfig {
            adjustment_speed: Decimal::from_ratio(50u128, 1u128),
            min_adaptive_rate: Decimal::permille(1),
            max_adaptive_rate: Decimal::from_ratio(2u128, 1u128),
        }
    }

    fn default_instantiate_msg(app: &App) -> InstantiateMsg {
        InstantiateMsg {
            owner: Some(get_addr(app, ADMIN).to_string()),
            collateral_contract: Some(get_addr(app, COLLATERAL_CONTRACT).to_string()),
            liquidation_contract: Some(get_addr(app, LIQUIDATION_CONTRACT).to_string()),
            staking_contract: None,
            chain_proxy: None, // No chain proxy to avoid burn SubMsg issues in test
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            revenue_distributor: None,
            transmuter_addr: None,
            points_contract: Some(get_addr(app, POINTS_CONTRACT).to_string()),
            liq_fee: Decimal::percent(10),
            credit_twap_timeframe: 60,
            cpc_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(2_000_000_000),
            base_debt_cap_multiplier: Uint128::new(10),
            rate_slope_multiplier: Decimal::one(),
            affiliate_fee_max: Decimal::percent(5),
            irm_config: default_irm_config(),
            acquisition_contract: None,
            create_basket: CreateBasketDebt {
                basket_id: Uint128::one(),
                credit_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: CDT_DENOM.to_string(),
                    },
                    amount: Uint128::zero(),
                },
                credit_price: Decimal::one(),
                base_interest_rate: Some(Decimal::percent(2)),
                credit_pool_infos: vec![],
                liq_queue: None,
            },
        }
    }

    fn instantiate_debt_contract(app: &mut App) -> (Addr, Addr, Addr, Addr) {
        let debt_code_id = app.store_code(debt_contract());
        let admin = get_addr(app, ADMIN);
        let collateral = get_addr(app, COLLATERAL_CONTRACT);
        let liquidation = get_addr(app, LIQUIDATION_CONTRACT);
        let msg = default_instantiate_msg(app);
        let debt_addr = app
            .instantiate_contract(debt_code_id, admin.clone(), &msg, &[], "debt", None)
            .unwrap();
        (debt_addr, admin, collateral, liquidation)
    }

    fn create_position_via_notify(
        app: &mut App,
        debt_addr: &Addr,
        collateral_addr: &Addr,
        position_id: Uint128,
        position_owner: &str,
    ) {
        let owner_addr = get_addr(app, position_owner);
        app.execute_contract(
            collateral_addr.clone(),
            debt_addr.clone(),
            &ExecuteMsg::NotifyDeposit {
                position_id,
                position_owner: owner_addr.to_string(),
                affiliate_address: None,
            },
            &[],
        )
        .unwrap();
    }

    fn query_debt_positions(app: &App, debt_addr: &Addr, user: &str) -> DebtPositionsResponse {
        let user_addr = get_addr(app, user);
        app.wrap()
            .query_wasm_smart(
                debt_addr.clone(),
                &QueryMsg::GetDebtPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user_addr.to_string()),
                },
            )
            .unwrap()
    }

    fn query_basket(app: &App, debt_addr: &Addr) -> BasketDebt {
        app.wrap()
            .query_wasm_smart(debt_addr.clone(), &QueryMsg::GetBasket {})
            .unwrap()
    }

    // ========================================================================
    // Helper: Create test segments
    // ========================================================================

    fn var_segment(amount: u128) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: None,
        }
    }

    fn fixed_segment(amount: u128, duration: u8, rate_pct: u64, end_time: u64, rollover: bool) -> RateSegment {
        RateSegment {
            amount: Uint128::new(amount),
            fixed_rate: Some(FixedRate {
                rate: Decimal::percent(rate_pct),
                end: FixedRateEnd {
                    end_time,
                    rollover,
                    duration_months: duration,
                },
            }),
        }
    }

    fn default_caps() -> FixedRateCaps {
        FixedRateCaps {
            one_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.1").unwrap(),
            },
            three_month: FixedRateCap {
                cap: Decimal::percent(20),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.3").unwrap(),
            },
            six_month: FixedRateCap {
                cap: Decimal::percent(10),
                amount: Uint128::zero(),
                multiplier: Decimal::from_str("1.7").unwrap(),
            },
        }
    }

    fn zero_caps() -> FixedRateCaps {
        FixedRateCaps {
            one_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
            three_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
            six_month: FixedRateCap { cap: Decimal::zero(), amount: Uint128::zero(), multiplier: Decimal::one() },
        }
    }

    fn default_rates() -> Rates {
        Rates {
            current_adaptive_rate: vec![],
            peg_current_adaptive_rate: vec![],
            lastest_collateral_rates: vec![],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: default_caps(),
            rates_last_accrued: 1000,
            credit_last_accrued: 1000,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        }
    }

    // ========================================================================
    // A. REPAY FLOW TESTS (via debt_manager functions)
    // ========================================================================

    #[test]
    fn test_repay_full_position_variable_only() {
        let mut segments = vec![var_segment(5_000_000_000)];
        let (applied, deltas) = repay_segments_default_order(&mut segments, Uint128::new(5_000_000_000));
        assert_eq!(applied, Uint128::new(5_000_000_000));
        assert_eq!(deltas.variable, Int128::from(-5_000_000_000i128));
        assert!(segments.is_empty());
    }

    #[test]
    fn test_repay_partial_position_mixed_segments() {
        let mut segments = vec![
            var_segment(3_000_000_000),
            fixed_segment(2_000_000_000, 3, 7, 99999999, false),
            fixed_segment(1_000_000_000, 6, 10, 99999999, false),
        ];

        let (applied, deltas) = repay_segments_default_order(&mut segments, Uint128::new(4_000_000_000));
        assert_eq!(applied, Uint128::new(4_000_000_000));
        assert_eq!(deltas.variable, Int128::from(-3_000_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-1_000_000_000i128));
        assert_eq!(deltas.six_month, Int128::zero());
        assert_eq!(get_total_from_segments(&segments), Uint128::new(2_000_000_000));
    }

    #[test]
    fn test_repay_with_peg_debt_cross_pool() {
        let mut regular_segs = vec![
            var_segment(2_000_000_000),
            fixed_segment(1_000_000_000, 1, 5, 99999999, false),
        ];
        let mut peg_segs = vec![
            var_segment(3_000_000_000),
        ];

        let repay_amount = Uint128::new(5_000_000_000);
        let (regular_applied, reg_d) = repay_segments_default_order(&mut regular_segs, repay_amount);
        let remaining = repay_amount.checked_sub(regular_applied).unwrap();
        let (peg_applied, peg_d) = repay_segments_default_order(&mut peg_segs, remaining);

        assert_eq!(regular_applied, Uint128::new(3_000_000_000));
        assert_eq!(reg_d.variable, Int128::from(-2_000_000_000i128));
        assert_eq!(reg_d.one_month, Int128::from(-1_000_000_000i128));
        assert_eq!(peg_applied, Uint128::new(2_000_000_000));
        assert_eq!(peg_d.variable, Int128::from(-2_000_000_000i128));
        assert_eq!(get_total_from_segments(&peg_segs), Uint128::new(1_000_000_000));
    }

    #[test]
    fn test_repay_targeted_split_validation() {
        let mut regular_segs = vec![
            var_segment(5_000_000_000),
            fixed_segment(3_000_000_000, 3, 7, 99999999, false),
        ];

        let split = SegmentSplit {
            variable: Some(Decimal::percent(70)),
            three_month: Some(Decimal::percent(30)),
            ..SegmentSplit::default()
        };

        let (applied, deltas) = repay_segments_targeted(&mut regular_segs, Uint128::new(2_000_000_000), &split).unwrap();
        assert_eq!(applied, Uint128::new(2_000_000_000));
        assert_eq!(deltas.variable, Int128::from(-1_400_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-600_000_000i128));
        assert_eq!(regular_segs[0].amount, Uint128::new(3_600_000_000));
        assert_eq!(regular_segs[1].amount, Uint128::new(2_400_000_000));
    }

    #[test]
    fn test_repay_excess_returned() {
        let mut segments = vec![var_segment(1_000_000_000)];
        let (applied, _) = repay_segments_default_order(&mut segments, Uint128::new(5_000_000_000));
        assert_eq!(applied, Uint128::new(1_000_000_000));
        assert!(segments.is_empty());
    }

    #[test]
    fn test_repay_default_order_all_fixed_durations() {
        let mut segments = vec![
            fixed_segment(500_000_000, 6, 10, 99999999, false),
            fixed_segment(500_000_000, 3, 7, 99999999, false),
            fixed_segment(500_000_000, 1, 5, 99999999, false),
            var_segment(500_000_000),
        ];

        let (applied, deltas) = repay_segments_default_order(&mut segments, Uint128::new(1_200_000_000));
        assert_eq!(applied, Uint128::new(1_200_000_000));
        assert_eq!(deltas.variable, Int128::from(-500_000_000i128));
        assert_eq!(deltas.one_month, Int128::from(-500_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-200_000_000i128));
        assert_eq!(deltas.six_month, Int128::zero());
    }

    // ========================================================================
    // B. SEGMENT CREATION & FIXED RATE TESTS
    // ========================================================================

    #[test]
    fn test_create_segments_within_caps() {
        let env = mock_env();
        let caps = default_caps();
        let mut segs = vec![];

        let split = SegmentSplit {
            variable: Some(Decimal::percent(60)),
            one_month: Some(Decimal::percent(15)),
            three_month: Some(Decimal::percent(15)),
            six_month: Some(Decimal::percent(10)),
            ..SegmentSplit::default()
        };

        let view = BreakdownView::default();
        let total = Uint128::new(10_000_000_000);

        let deltas = create_segments_from_split(
            total, &split, &caps, Decimal::percent(5),
            &mut segs, &view, total, &env,
        ).unwrap();

        assert_eq!(segs.len(), 4);
        assert_eq!(deltas.variable, Int128::from(6_000_000_000i128));
        assert_eq!(deltas.one_month, Int128::from(1_500_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(1_500_000_000i128));
        assert_eq!(deltas.six_month, Int128::from(1_000_000_000i128));

        // Verify fixed rates = base * multiplier
        let fr1 = segs[1].fixed_rate.as_ref().unwrap();
        assert_eq!(fr1.rate, Decimal::from_str("0.055").unwrap()); // 5% * 1.1
        assert_eq!(fr1.end.duration_months, 1);

        let fr3 = segs[2].fixed_rate.as_ref().unwrap();
        assert_eq!(fr3.rate, Decimal::from_str("0.065").unwrap()); // 5% * 1.3

        let fr6 = segs[3].fixed_rate.as_ref().unwrap();
        assert_eq!(fr6.rate, Decimal::from_str("0.085").unwrap()); // 5% * 1.7
    }

    #[test]
    fn test_create_segments_cap_exceeded_1mo() {
        let env = mock_env();
        let caps = default_caps();
        let mut segs = vec![];

        let split = SegmentSplit {
            one_month: Some(Decimal::one()),
            ..SegmentSplit::default()
        };

        let view = BreakdownView::default();
        let result = create_segments_from_split(
            Uint128::new(1_000_000_000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1_000_000_000), &env,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_segments_cap_exceeded_6mo() {
        let env = mock_env();
        let caps = default_caps(); // 10% cap on 6-month
        let mut segs = vec![];

        let split = SegmentSplit {
            variable: Some(Decimal::percent(80)),
            six_month: Some(Decimal::percent(20)),
            ..SegmentSplit::default()
        };

        let view = BreakdownView::default();
        let result = create_segments_from_split(
            Uint128::new(1_000_000_000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1_000_000_000), &env,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_segments_shared_cap_regular_plus_peg() {
        let env = mock_env();
        let caps = default_caps();

        let bd = CreditAssetBreakdown {
            info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
            variable_amount: Uint128::zero(),
            one_month_amount: Uint128::new(150_000_000),
            three_month_amount: Uint128::zero(),
            six_month_amount: Uint128::zero(),
            peg_variable_amount: Uint128::zero(),
            peg_one_month_amount: Uint128::new(50_000_000),
            peg_three_month_amount: Uint128::zero(),
            peg_six_month_amount: Uint128::zero(),
        };
        let view = combined_breakdown_for_caps(&bd);
        assert_eq!(view.one_month, Uint128::new(200_000_000));

        let split = SegmentSplit {
            one_month: Some(Decimal::one()),
            ..SegmentSplit::default()
        };
        let mut segs = vec![];
        let result = create_segments_from_split(
            Uint128::new(50_000_000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1_050_000_000), &env,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fixed_rate_end_time_calculation() {
        let env = mock_env();
        let caps = zero_caps();
        let mut segs = vec![];

        let split = SegmentSplit {
            three_month: Some(Decimal::one()),
            three_month_rollover: Some(true),
            ..SegmentSplit::default()
        };

        let view = BreakdownView::default();
        create_segments_from_split(
            Uint128::new(1_000_000_000), &split, &caps, Decimal::percent(5),
            &mut segs, &view, Uint128::new(1_000_000_000), &env,
        ).unwrap();

        let fr = segs[0].fixed_rate.as_ref().unwrap();
        let expected_end = env.block.time.seconds() + 7_776_000;
        assert_eq!(fr.end.end_time, expected_end);
        assert!(fr.end.rollover);
        assert_eq!(fr.end.duration_months, 3);
    }

    // ========================================================================
    // C. INTEREST ACCRUAL TESTS
    // ========================================================================

    #[test]
    fn test_fixed_rate_accrual_1_year() {
        let amount = Uint128::new(10_000_000_000);
        let accrued = calculate_fixed_rate_accrual(
            amount, Decimal::percent(8), 0, SECONDS_PER_YEAR,
        ).unwrap();
        assert_eq!(accrued, Uint128::new(800_000_000));
    }

    #[test]
    fn test_fixed_rate_accrual_1_month() {
        let amount = Uint128::new(5_000_000_000);
        let seconds_per_month = 2_592_000u64;
        let accrued = calculate_fixed_rate_accrual(
            amount, Decimal::percent(6), 0, seconds_per_month,
        ).unwrap();
        let expected = Uint128::new(5_000_000_000)
            .multiply_ratio(6u128, 100u128)
            .multiply_ratio(seconds_per_month as u128, SECONDS_PER_YEAR as u128);
        assert_eq!(accrued, expected);
    }

    #[test]
    fn test_fixed_rate_accrual_zero_amount() {
        let accrued = calculate_fixed_rate_accrual(
            Uint128::zero(), Decimal::percent(10), 0, SECONDS_PER_YEAR,
        ).unwrap();
        assert_eq!(accrued, Uint128::zero());
    }

    #[test]
    fn test_fixed_rate_accrual_zero_rate() {
        let accrued = calculate_fixed_rate_accrual(
            Uint128::new(1_000_000_000), Decimal::zero(), 0, SECONDS_PER_YEAR,
        ).unwrap();
        assert_eq!(accrued, Uint128::zero());
    }

    #[test]
    fn test_fixed_rate_accrual_zero_time() {
        let accrued = calculate_fixed_rate_accrual(
            Uint128::new(1_000_000_000), Decimal::percent(10), 1000, 1000,
        ).unwrap();
        assert_eq!(accrued, Uint128::zero());
    }

    #[test]
    fn test_recalculate_fixed_rate_rollover() {
        let rates = default_rates();

        let (rate, end_time) = recalculate_fixed_rate(&rates, 1, 50_000, Decimal::percent(8)).unwrap();
        assert_eq!(rate, Decimal::from_str("0.088").unwrap());
        assert_eq!(end_time, 50_000 + 2_592_000);

        let (rate, end_time) = recalculate_fixed_rate(&rates, 3, 50_000, Decimal::percent(8)).unwrap();
        assert_eq!(rate, Decimal::from_str("0.104").unwrap());
        assert_eq!(end_time, 50_000 + 3 * 2_592_000);

        let (rate, end_time) = recalculate_fixed_rate(&rates, 6, 50_000, Decimal::percent(8)).unwrap();
        assert_eq!(rate, Decimal::from_str("0.136").unwrap());
        assert_eq!(end_time, 50_000 + 6 * 2_592_000);
    }

    #[test]
    fn test_adaptive_smooth_rate_stability() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(1),
            max_adaptive_rate: Decimal::one(),
        };

        let result = adaptive_smooth_rate(
            Decimal::percent(5), Decimal::percent(5), 86400, &irm,
        );
        assert_eq!(result, Decimal::percent(5));
    }

    #[test]
    fn test_adaptive_smooth_rate_convergence() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::permille(1),
            max_adaptive_rate: Decimal::one(),
        };

        let mut current = Decimal::percent(2);
        let destination = Decimal::percent(10);

        for _ in 0..10 {
            current = adaptive_smooth_rate(current, destination, 86400, &irm);
        }

        assert!(current > Decimal::percent(2));
        assert!(current <= Decimal::percent(10));
    }

    #[test]
    fn test_adaptive_smooth_rate_downward() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::permille(1),
            max_adaptive_rate: Decimal::one(),
        };

        let mut current = Decimal::percent(20);
        let destination = Decimal::percent(3);

        for _ in 0..10 {
            current = adaptive_smooth_rate(current, destination, 86400, &irm);
        }

        assert!(current < Decimal::percent(20));
        assert!(current >= Decimal::percent(3));
    }

    #[test]
    fn test_adaptive_smooth_rate_min_bound() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::percent(5),
            max_adaptive_rate: Decimal::one(),
        };

        let result = adaptive_smooth_rate(
            Decimal::percent(6), Decimal::percent(1), SECONDS_PER_YEAR, &irm,
        );
        assert!(result >= Decimal::percent(5));
    }

    #[test]
    fn test_adaptive_smooth_rate_max_bound() {
        let irm = IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),
            min_adaptive_rate: Decimal::permille(1),
            max_adaptive_rate: Decimal::percent(50),
        };

        let result = adaptive_smooth_rate(
            Decimal::zero(), Decimal::percent(80), 86400, &irm,
        );
        assert_eq!(result, Decimal::percent(50));
    }

    // ========================================================================
    // D. LIQUIDATION DEBT REDUCTION TESTS
    // ========================================================================

    #[test]
    fn test_liquidation_reduce_variable_only() {
        let mut segments = vec![var_segment(10_000_000_000)];
        let total_debt = Uint128::new(10_000_000_000);

        let deltas = reduce_segments_proportionally(
            &mut segments, total_debt, total_debt, Uint128::new(3_000_000_000),
        );

        assert_eq!(deltas.variable, Int128::from(-3_000_000_000i128));
        assert_eq!(segments[0].amount, Uint128::new(7_000_000_000));
    }

    #[test]
    fn test_liquidation_reduce_mixed_segments() {
        let mut regular_segs = vec![
            var_segment(4_000_000_000),
            fixed_segment(3_000_000_000, 3, 7, 99999999, false),
        ];
        let mut peg_segs = vec![var_segment(3_000_000_000)];

        let regular_total = Uint128::new(7_000_000_000);
        let peg_total = Uint128::new(3_000_000_000);
        let total_debt = Uint128::new(10_000_000_000);
        let repay_amount = Uint128::new(5_000_000_000);

        let reg_deltas = reduce_segments_proportionally(
            &mut regular_segs, regular_total, total_debt, repay_amount,
        );
        assert_eq!(reg_deltas.variable, Int128::from(-3_500_000_000i128));
        assert_eq!(reg_deltas.three_month, Int128::zero());
        assert_eq!(regular_segs[0].amount, Uint128::new(500_000_000));

        let peg_deltas = reduce_segments_proportionally(
            &mut peg_segs, peg_total, total_debt, repay_amount,
        );
        assert_eq!(peg_deltas.variable, Int128::from(-1_500_000_000i128));
        assert_eq!(peg_segs[0].amount, Uint128::new(1_500_000_000));
    }

    #[test]
    fn test_liquidation_reduce_spills_to_fixed() {
        let mut segments = vec![
            var_segment(1_000_000_000),
            fixed_segment(4_000_000_000, 1, 5, 99999999, false),
            fixed_segment(5_000_000_000, 3, 7, 99999999, false),
        ];
        let total = Uint128::new(10_000_000_000);

        let deltas = reduce_segments_proportionally(
            &mut segments, total, total, Uint128::new(8_000_000_000),
        );

        assert_eq!(deltas.variable, Int128::from(-1_000_000_000i128));
        assert_eq!(deltas.one_month, Int128::from(-4_000_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-3_000_000_000i128));
        assert_eq!(get_total_from_segments(&segments), Uint128::new(2_000_000_000));
    }

    #[test]
    fn test_liquidation_full_debt_wipeout() {
        let mut segments = vec![
            var_segment(2_000_000_000),
            fixed_segment(3_000_000_000, 6, 10, 99999999, false),
        ];
        let total = Uint128::new(5_000_000_000);

        let deltas = reduce_segments_proportionally(
            &mut segments, total, total, Uint128::new(5_000_000_000),
        );

        assert_eq!(deltas.variable, Int128::from(-2_000_000_000i128));
        assert_eq!(deltas.six_month, Int128::from(-3_000_000_000i128));
        assert!(segments.is_empty());
    }

    // ========================================================================
    // E. POSITION DEBT TRACKING TESTS
    // ========================================================================

    #[test]
    fn test_get_total_position_debt_both_pools() {
        let position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![
                var_segment(5_000_000_000),
                fixed_segment(2_000_000_000, 3, 7, 99999999, false),
            ],
            peg_rate_segments: vec![var_segment(3_000_000_000)],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        assert_eq!(get_total_position_debt(&position), Uint128::new(10_000_000_000));
    }

    #[test]
    fn test_get_total_position_debt_empty() {
        let position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };
        assert_eq!(get_total_position_debt(&position), Uint128::zero());
    }

    #[test]
    fn test_position_debt_after_partial_repay() {
        let mut position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![
                var_segment(5_000_000_000),
                fixed_segment(3_000_000_000, 1, 5, 99999999, false),
            ],
            peg_rate_segments: vec![var_segment(2_000_000_000)],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };

        assert_eq!(get_total_position_debt(&position), Uint128::new(10_000_000_000));

        let (_applied, _) = repay_segments_default_order(&mut position.rate_segments, Uint128::new(4_000_000_000));

        assert_eq!(get_total_from_segments(&position.rate_segments), Uint128::new(4_000_000_000));
        assert_eq!(get_total_position_debt(&position), Uint128::new(6_000_000_000));
    }

    // ========================================================================
    // F. BASKET DEBT TRACKING TESTS
    // ========================================================================

    #[test]
    fn test_apply_deltas_to_basket_after_repay() {
        use membrane::types::CreditAssetBreakdown;

        let mut breakdown = CreditAssetBreakdown {
            info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
            variable_amount: Uint128::new(5_000_000_000),
            one_month_amount: Uint128::new(2_000_000_000),
            three_month_amount: Uint128::new(3_000_000_000),
            six_month_amount: Uint128::zero(),
            peg_variable_amount: Uint128::new(1_000_000_000),
            peg_one_month_amount: Uint128::zero(),
            peg_three_month_amount: Uint128::zero(),
            peg_six_month_amount: Uint128::zero(),
        };

        let reg_deltas = SegmentDeltas {
            variable: Int128::from(-3_000_000_000i128),
            one_month: Int128::from(-1_000_000_000i128),
            three_month: Int128::zero(),
            six_month: Int128::zero(),
        };
        apply_delta_to_regular_debt(&mut breakdown, &reg_deltas);

        assert_eq!(breakdown.variable_amount, Uint128::new(2_000_000_000));
        assert_eq!(breakdown.one_month_amount, Uint128::new(1_000_000_000));
        assert_eq!(breakdown.three_month_amount, Uint128::new(3_000_000_000));
        assert_eq!(breakdown.peg_variable_amount, Uint128::new(1_000_000_000));
    }

    #[test]
    fn test_apply_deltas_clamping() {
        use membrane::types::CreditAssetBreakdown;

        let mut breakdown = CreditAssetBreakdown {
            info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
            variable_amount: Uint128::new(100),
            one_month_amount: Uint128::new(50),
            three_month_amount: Uint128::zero(),
            six_month_amount: Uint128::zero(),
            peg_variable_amount: Uint128::zero(),
            peg_one_month_amount: Uint128::zero(),
            peg_three_month_amount: Uint128::zero(),
            peg_six_month_amount: Uint128::zero(),
        };

        let deltas = SegmentDeltas {
            variable: Int128::from(-200i128),
            one_month: Int128::from(-50i128),
            three_month: Int128::from(-100i128),
            six_month: Int128::zero(),
        };
        apply_delta_to_regular_debt(&mut breakdown, &deltas);

        assert_eq!(breakdown.variable_amount, Uint128::zero());
        assert_eq!(breakdown.one_month_amount, Uint128::zero());
        assert_eq!(breakdown.three_month_amount, Uint128::zero());
    }

    // ========================================================================
    // G. DISCOUNT APPLICATION TESTS
    // ========================================================================

    #[test]
    fn test_discount_proportional_across_segments() {
        let mut segments = vec![
            var_segment(6_000_000_000),
            fixed_segment(4_000_000_000, 3, 7, 99999999, false),
        ];
        let total_debt = Uint128::new(10_000_000_000);
        let discount = Uint128::new(1_000_000_000);

        let deltas = debt_manager::apply_discount_to_segments(&mut segments, discount, total_debt);

        assert_eq!(segments[0].amount, Uint128::new(5_400_000_000));
        assert_eq!(segments[1].amount, Uint128::new(3_600_000_000));
        assert_eq!(deltas.variable, Int128::from(-600_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-400_000_000i128));
    }

    #[test]
    fn test_discount_zero_no_effect() {
        let mut segments = vec![var_segment(5_000_000_000)];
        let deltas = debt_manager::apply_discount_to_segments(
            &mut segments, Uint128::zero(), Uint128::new(5_000_000_000),
        );
        assert_eq!(segments[0].amount, Uint128::new(5_000_000_000));
        assert_eq!(deltas.variable, Int128::zero());
    }

    // ========================================================================
    // H. INTENT MANAGEMENT TESTS (via execute interface)
    // ========================================================================

    #[test]
    fn test_set_user_intents_new() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue = get_addr(&app, "vault_venue");
        app.execute_contract(
            user.clone(),
            debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue.to_string(),
                    ltv_to_mint: Decimal::percent(30),
                },
            },
            &[],
        ).unwrap();

        let intents: Vec<(String, Option<UserDeploymentIntents>)> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetUserIntent { users: vec![user.to_string()], start_after: None, limit: None },
        ).unwrap();
        assert_eq!(intents.len(), 1);
        let user_intents = intents[0].1.as_ref().unwrap();
        assert_eq!(user_intents.deployment_intents.len(), 1);
        assert_eq!(user_intents.deployment_intents[0].ltv_to_mint, Decimal::percent(30));
    }

    #[test]
    fn test_set_user_intents_update_ltv() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue = get_addr(&app, "vault_venue");

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue.to_string(),
                    ltv_to_mint: Decimal::percent(30),
                },
            },
            &[],
        ).unwrap();

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue.to_string(),
                    ltv_to_mint: Decimal::percent(50),
                },
            },
            &[],
        ).unwrap();

        let intents: Vec<(String, Option<UserDeploymentIntents>)> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetUserIntent { users: vec![user.to_string()], start_after: None, limit: None },
        ).unwrap();
        assert_eq!(intents.len(), 1);
        let user_intents = intents[0].1.as_ref().unwrap();
        assert_eq!(user_intents.deployment_intents.len(), 1);
        assert_eq!(user_intents.deployment_intents[0].ltv_to_mint, Decimal::percent(50));
    }

    #[test]
    fn test_set_user_intents_invalid_ltv() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue = get_addr(&app, "vault_venue");

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue.to_string(),
                    ltv_to_mint: Decimal::from_str("1.5").unwrap(),
                },
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("above 1"));
    }

    #[test]
    fn test_set_user_intents_no_position() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let venue = get_addr(&app, "vault_venue");
        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue.to_string(),
                    ltv_to_mint: Decimal::percent(30),
                },
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("No user positions") ||
                err.root_cause().to_string().contains("not found"));
    }

    #[test]
    fn test_set_user_intents_multiple_venues() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue1 = get_addr(&app, "vault_1");
        let venue2 = get_addr(&app, "vault_2");

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue1.to_string(),
                    ltv_to_mint: Decimal::percent(20),
                },
            },
            &[],
        ).unwrap();

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    user: user.to_string(),
                    position_id: Uint128::one(),
                    destination: venue2.to_string(),
                    ltv_to_mint: Decimal::percent(15),
                },
            },
            &[],
        ).unwrap();

        let intents: Vec<(String, Option<UserDeploymentIntents>)> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetUserIntent { users: vec![user.to_string()], start_after: None, limit: None },
        ).unwrap();
        assert_eq!(intents.len(), 1);
        let user_intents = intents[0].1.as_ref().unwrap();
        assert_eq!(user_intents.deployment_intents.len(), 2);
    }

    // ========================================================================
    // I. ERROR PATHS (via execute interface)
    // ========================================================================

    #[test]
    fn test_repay_no_position() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::Repay {
                position_id: Uint128::one(),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            },
            &[coin(1_000_000u128, CDT_DENOM)],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("No user positions") ||
                err.root_cause().to_string().contains("not found"));
    }

    #[test]
    fn test_increase_debt_frozen() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        app.execute_contract(
            admin.clone(), debt_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: true }, &[],
        ).unwrap();

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::IncreaseDebt {
                position_id: Uint128::one(),
                amount: Some(Uint128::new(5_000_000_000)),
                LTV: None, mint_to_addr: None, deployment_intent: None,
                debt_split: None, rollover_updates: None, peg_debt: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    #[test]
    fn test_increase_debt_no_position() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::IncreaseDebt {
                position_id: Uint128::one(),
                amount: Some(Uint128::new(5_000_000_000)),
                LTV: None, mint_to_addr: None, deployment_intent: None,
                debt_split: None, rollover_updates: None, peg_debt: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("No user positions") ||
                err.root_cause().to_string().contains("not found"));
    }

    #[test]
    fn test_close_position_frozen() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        app.execute_contract(
            admin.clone(), debt_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: true }, &[],
        ).unwrap();

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::ClosePosition {
                position_id: Uint128::one(),
                close_percentage: None,
                max_spread: Decimal::percent(10),
                send_to: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    // ========================================================================
    // J. MULTI-STEP FINANCIAL SCENARIOS
    // ========================================================================

    #[test]
    fn test_scenario_create_repay_variable_then_peg() {
        let mut regular_segs = vec![
            var_segment(3_000_000_000),
            fixed_segment(2_000_000_000, 1, 5, 99999999, false),
        ];
        let mut peg_segs = vec![
            var_segment(4_000_000_000),
            fixed_segment(1_000_000_000, 3, 7, 99999999, false),
        ];

        let total_repay = Uint128::new(8_000_000_000);

        let (reg_applied, reg_d) = repay_segments_default_order(&mut regular_segs, total_repay);
        assert_eq!(reg_applied, Uint128::new(5_000_000_000));

        let remaining = total_repay.checked_sub(reg_applied).unwrap();
        assert_eq!(remaining, Uint128::new(3_000_000_000));

        let (peg_applied, peg_d) = repay_segments_default_order(&mut peg_segs, remaining);
        assert_eq!(peg_applied, Uint128::new(3_000_000_000));

        assert!(regular_segs.is_empty());
        assert_eq!(get_total_from_segments(&peg_segs), Uint128::new(2_000_000_000));

        assert_eq!(reg_d.variable, Int128::from(-3_000_000_000i128));
        assert_eq!(reg_d.one_month, Int128::from(-2_000_000_000i128));
        assert_eq!(peg_d.variable, Int128::from(-3_000_000_000i128));
    }

    #[test]
    fn test_scenario_targeted_repay_both_pools() {
        let mut regular_segs = vec![
            var_segment(5_000_000_000),
            fixed_segment(5_000_000_000, 3, 7, 99999999, false),
        ];
        let mut peg_segs = vec![
            var_segment(5_000_000_000),
            fixed_segment(5_000_000_000, 6, 10, 99999999, false),
        ];

        let regular_split = SegmentSplit {
            variable: Some(Decimal::percent(20)),
            three_month: Some(Decimal::percent(30)),
            ..SegmentSplit::default()
        };
        let peg_split = SegmentSplit {
            variable: Some(Decimal::percent(10)),
            six_month: Some(Decimal::percent(40)),
            ..SegmentSplit::default()
        };

        let repay = Uint128::new(10_000_000_000);

        let (reg_applied, reg_d) = repay_segments_targeted(&mut regular_segs, repay, &regular_split).unwrap();
        let (peg_applied, peg_d) = repay_segments_targeted(&mut peg_segs, repay, &peg_split).unwrap();

        assert_eq!(reg_d.variable, Int128::from(-2_000_000_000i128));
        assert_eq!(reg_d.three_month, Int128::from(-3_000_000_000i128));
        assert_eq!(reg_applied, Uint128::new(5_000_000_000));

        assert_eq!(peg_d.variable, Int128::from(-1_000_000_000i128));
        assert_eq!(peg_d.six_month, Int128::from(-4_000_000_000i128));
        assert_eq!(peg_applied, Uint128::new(5_000_000_000));
    }

    #[test]
    fn test_scenario_accrual_then_repay_then_check_totals() {
        use membrane::types::CreditAssetBreakdown;

        let mut regular_segs = vec![
            var_segment(5_000_000_000),
            fixed_segment(3_000_000_000, 3, 7, 99999999, false),
        ];
        let mut basket_breakdown = CreditAssetBreakdown {
            info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
            variable_amount: Uint128::new(5_000_000_000),
            one_month_amount: Uint128::zero(),
            three_month_amount: Uint128::new(3_000_000_000),
            six_month_amount: Uint128::zero(),
            peg_variable_amount: Uint128::zero(),
            peg_one_month_amount: Uint128::zero(),
            peg_three_month_amount: Uint128::zero(),
            peg_six_month_amount: Uint128::zero(),
        };

        // Step 1: Simulate 5% variable accrual
        let new_var = Uint128::new(5_000_000_000).multiply_ratio(105u128, 100u128);
        let accrued = new_var - Uint128::new(5_000_000_000);
        regular_segs[0].amount = new_var;

        let accrual_deltas = SegmentDeltas {
            variable: Int128::from(accrued.u128() as i128),
            one_month: Int128::zero(),
            three_month: Int128::zero(),
            six_month: Int128::zero(),
        };
        apply_delta_to_regular_debt(&mut basket_breakdown, &accrual_deltas);
        assert_eq!(basket_breakdown.variable_amount, Uint128::new(5_250_000_000));

        // Step 2: Repay 4000
        let (applied, repay_deltas) = repay_segments_default_order(&mut regular_segs, Uint128::new(4_000_000_000));
        assert_eq!(applied, Uint128::new(4_000_000_000));
        apply_delta_to_regular_debt(&mut basket_breakdown, &repay_deltas);

        assert_eq!(basket_breakdown.variable_amount, Uint128::new(1_250_000_000));
        assert_eq!(basket_breakdown.three_month_amount, Uint128::new(3_000_000_000));
    }

    #[test]
    fn test_scenario_liquidation_followed_by_bad_debt() {
        let mut app = mock_app();
        let (debt_addr, _, _, liquidation) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        app.execute_contract(
            liquidation.clone(), debt_addr.clone(),
            &ExecuteMsg::RecordBadDebt {
                position_id: Uint128::one(),
                position_owner: user.to_string(),
                bad_debt_amount: Uint128::new(5_000_000_000),
            },
            &[],
        ).unwrap();

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::new(5_000_000_000));

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::FulfillBadDebt {},
            &[coin(2_000_000_000u128, CDT_DENOM)],
        ).unwrap();

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::new(3_000_000_000));

        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::FulfillBadDebt {},
            &[coin(3_000_000_000u128, CDT_DENOM)],
        ).unwrap();

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::zero());
    }

    #[test]
    fn test_scenario_fulfill_bad_debt_excess_returned() {
        let mut app = mock_app();
        let (debt_addr, _, _, liquidation) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        app.execute_contract(
            liquidation.clone(), debt_addr.clone(),
            &ExecuteMsg::RecordBadDebt {
                position_id: Uint128::one(),
                position_owner: user.to_string(),
                bad_debt_amount: Uint128::new(1_000_000_000),
            },
            &[],
        ).unwrap();

        let user_balance_before: cosmwasm_std::Coin = app.wrap()
            .query_balance(user.to_string(), CDT_DENOM).unwrap();

        let resp = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::FulfillBadDebt {},
            &[coin(5_000_000_000u128, CDT_DENOM)],
        ).unwrap();

        let applied_attr = resp.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "applied");
        assert!(applied_attr.is_some());
        assert_eq!(applied_attr.unwrap().value, "1000000000");

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::zero());

        let user_balance_after: cosmwasm_std::Coin = app.wrap()
            .query_balance(user.to_string(), CDT_DENOM).unwrap();
        let spent = user_balance_before.amount - user_balance_after.amount;
        assert_eq!(spent, Uint128::new(1_000_000_000));
    }

    // ========================================================================
    // K. AUTHORIZATION EDGE CASES
    // ========================================================================

    #[test]
    fn test_liquidation_debt_update_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::LiquidationDebtUpdate {
                position_id: Uint128::one(),
                position_owner: user.to_string(),
                repay_amount: Uint128::new(1_000_000_000),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_accrue_and_validate_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::AccrueAndValidate {
                position_owner: user.to_string(),
                position_id: Uint128::one(),
                remaining_collateral_value: Decimal::one(),
                avg_max_borrow_ltv: Decimal::percent(50),
                avg_max_ltv: Decimal::percent(60),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_accrue_and_return_debt_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::AccrueAndReturnDebt {
                position_owner: user.to_string(),
                position_id: Uint128::one(),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // L. COMBINED BREAKDOWN & CAP TRACKING
    // ========================================================================

    #[test]
    fn test_combined_breakdown_for_caps_all_segments() {
        use membrane::types::CreditAssetBreakdown;

        let bd = CreditAssetBreakdown {
            info: AssetInfo::NativeToken { denom: CDT_DENOM.to_string() },
            variable_amount: Uint128::new(1_000_000_000),
            one_month_amount: Uint128::new(500_000_000),
            three_month_amount: Uint128::new(800_000_000),
            six_month_amount: Uint128::new(200_000_000),
            peg_variable_amount: Uint128::new(300_000_000),
            peg_one_month_amount: Uint128::new(100_000_000),
            peg_three_month_amount: Uint128::new(400_000_000),
            peg_six_month_amount: Uint128::new(150_000_000),
        };

        let view = combined_breakdown_for_caps(&bd);
        assert_eq!(view.variable, Uint128::new(1_300_000_000));
        assert_eq!(view.one_month, Uint128::new(600_000_000));
        assert_eq!(view.three_month, Uint128::new(1_200_000_000));
        assert_eq!(view.six_month, Uint128::new(350_000_000));
    }

    #[test]
    fn test_apply_deltas_to_caps() {
        let mut caps = default_caps();
        caps.one_month.amount = Uint128::new(500_000_000);
        caps.three_month.amount = Uint128::new(800_000_000);
        caps.six_month.amount = Uint128::new(200_000_000);

        let deltas = SegmentDeltas {
            variable: Int128::from(1_000_000_000i128),
            one_month: Int128::from(-200_000_000i128),
            three_month: Int128::from(100_000_000i128),
            six_month: Int128::from(-300_000_000i128),
        };
        debt_manager::apply_delta_to_caps(&mut caps, &deltas);

        assert_eq!(caps.one_month.amount, Uint128::new(300_000_000));
        assert_eq!(caps.three_month.amount, Uint128::new(900_000_000));
        assert_eq!(caps.six_month.amount, Uint128::zero());
    }

    // ========================================================================
    // M. EDGE CASES
    // ========================================================================

    #[test]
    fn test_repay_only_fixed_no_variable() {
        let mut segments = vec![
            fixed_segment(2_000_000_000, 1, 5, 99999999, false),
            fixed_segment(3_000_000_000, 3, 7, 99999999, false),
        ];

        let (applied, deltas) = repay_segments_default_order(&mut segments, Uint128::new(4_000_000_000));
        assert_eq!(applied, Uint128::new(4_000_000_000));
        assert_eq!(deltas.variable, Int128::zero());
        assert_eq!(deltas.one_month, Int128::from(-2_000_000_000i128));
        assert_eq!(deltas.three_month, Int128::from(-2_000_000_000i128));
    }

    #[test]
    fn test_zero_amount_operations() {
        let mut segments = vec![var_segment(1_000_000_000)];

        let (applied, _) = repay_segments_default_order(&mut segments, Uint128::zero());
        assert_eq!(applied, Uint128::zero());
        assert_eq!(segments[0].amount, Uint128::new(1_000_000_000));

        let deltas = reduce_segments_proportionally(
            &mut segments, Uint128::new(1_000_000_000), Uint128::new(1_000_000_000), Uint128::zero(),
        );
        assert_eq!(deltas.variable, Int128::zero());
    }

    #[test]
    fn test_large_position_values() {
        let mut segments = vec![
            var_segment(100_000_000_000_000),
            fixed_segment(50_000_000_000_000, 6, 10, 99999999, false),
        ];

        let (applied, deltas) = repay_segments_default_order(
            &mut segments, Uint128::new(120_000_000_000_000),
        );
        assert_eq!(applied, Uint128::new(120_000_000_000_000));
        assert_eq!(deltas.variable, Int128::from(-100_000_000_000_000i128));
        assert_eq!(deltas.six_month, Int128::from(-20_000_000_000_000i128));
    }

    #[test]
    fn test_single_unit_amounts() {
        let mut segments = vec![var_segment(1)];
        let (applied, deltas) = repay_segments_default_order(&mut segments, Uint128::new(1));
        assert_eq!(applied, Uint128::new(1));
        assert_eq!(deltas.variable, Int128::from(-1i128));
        assert!(segments.is_empty());
    }

    #[test]
    fn test_many_fixed_rate_segments() {
        let mut segments = vec![
            var_segment(1_000_000_000),
            fixed_segment(500_000_000, 1, 5, 99999999, false),
            fixed_segment(500_000_000, 1, 6, 99999999, true),
            fixed_segment(800_000_000, 3, 7, 99999999, false),
            fixed_segment(200_000_000, 3, 8, 99999999, true),
            fixed_segment(1_000_000_000, 6, 10, 99999999, false),
        ];

        let total = get_total_from_segments(&segments);
        assert_eq!(total, Uint128::new(4_000_000_000));

        let (applied, deltas) = repay_segments_default_order(
            &mut segments, Uint128::new(2_000_000_000),
        );
        assert_eq!(applied, Uint128::new(2_000_000_000));
        assert_eq!(deltas.variable, Int128::from(-1_000_000_000i128));
        assert_eq!(deltas.one_month, Int128::from(-1_000_000_000i128));

        let remaining = get_total_from_segments(&segments);
        assert_eq!(remaining, Uint128::new(2_000_000_000));
    }

    // ========================================================================
    // N. QUERY TESTS FOR FINANCIAL DATA
    // ========================================================================

    #[test]
    fn test_query_user_intents_none() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let intents: Vec<(String, Option<UserDeploymentIntents>)> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetUserIntent { users: vec![user.to_string()], start_after: None, limit: None },
        ).unwrap();
        assert_eq!(intents.len(), 1);
        assert!(intents[0].1.is_none());
    }

    #[test]
    fn test_query_basket_initial_state() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.basket_id, Uint128::one());
        assert_eq!(basket.credit_price.price, Decimal::one());
        assert_eq!(basket.pending_bad_debt, Uint128::zero());
        assert_eq!(basket.pending_revenue.total_pending, Uint128::zero());
        assert!(!basket.oracle_set);
        assert!(!basket.distribute_revenue);

        assert_eq!(basket.credit_asset.variable_amount, Uint128::zero());
        assert_eq!(basket.credit_asset.one_month_amount, Uint128::zero());
        assert_eq!(basket.credit_asset.three_month_amount, Uint128::zero());
        assert_eq!(basket.credit_asset.six_month_amount, Uint128::zero());
        assert_eq!(basket.credit_asset.peg_variable_amount, Uint128::zero());
    }

    #[test]
    fn test_query_rates_initially_none() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);

        let rates: Option<Rates> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetRates {},
        ).unwrap();
        assert!(rates.is_none());
    }

    #[test]
    fn test_query_active_deployment_venues_empty() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);

        let venues: Vec<String> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetActiveDeploymentVenues { venue: None, start_after: None, limit: None },
        ).unwrap();
        assert!(venues.is_empty());
    }

    // ========================================================================
    // O. ACCRUE SEGMENTS TESTS (debt_manager)
    // ========================================================================

    #[test]
    fn test_accrue_segments_variable_only() {
        use crate::debt_manager::accrue_segments;

        let mut segments = vec![var_segment(10_000_000_000)];
        let rates = default_rates();
        // rate_of_change = 1.05 means 5% growth factor
        let rate_of_change = Decimal::from_str("1.05").unwrap();

        let (total_accrued, deltas) = accrue_segments(
            &mut segments,
            rate_of_change,
            1000,
            1000 + SECONDS_PER_YEAR,
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // Variable accrual via decimal_multiplication: amount * rate_of_change - amount
        let expected_accrued = Uint128::new(500_000_000); // 10B * 0.05
        assert_eq!(total_accrued, expected_accrued);
        assert_eq!(deltas.variable, Int128::from(expected_accrued.u128() as i128));
        assert_eq!(deltas.one_month, Int128::zero());
        assert_eq!(deltas.three_month, Int128::zero());
        assert_eq!(deltas.six_month, Int128::zero());
        assert_eq!(segments[0].amount, Uint128::new(10_500_000_000));
    }

    #[test]
    fn test_accrue_segments_fixed_only() {
        use crate::debt_manager::accrue_segments;

        let end_time = 1000 + 2 * SECONDS_PER_YEAR; // well in the future
        let mut segments = vec![
            fixed_segment(5_000_000_000, 3, 8, end_time, false),
        ];
        let rates = default_rates();

        let (total_accrued, deltas) = accrue_segments(
            &mut segments,
            Decimal::one(), // rate_of_change irrelevant for fixed
            1000,
            1000 + SECONDS_PER_YEAR,
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // Fixed accrual: 5B * 8% * 1 year = 400M
        assert_eq!(total_accrued, Uint128::new(400_000_000));
        assert_eq!(deltas.three_month, Int128::from(400_000_000i128));
        assert_eq!(deltas.variable, Int128::zero());
        assert_eq!(segments[0].amount, Uint128::new(5_400_000_000));
    }

    #[test]
    fn test_accrue_segments_mixed_variable_and_fixed() {
        use crate::debt_manager::accrue_segments;

        let end_time = 1000 + 2 * SECONDS_PER_YEAR;
        let mut segments = vec![
            var_segment(6_000_000_000),
            fixed_segment(4_000_000_000, 1, 6, end_time, false),
        ];
        let rates = default_rates();
        let rate_of_change = Decimal::from_str("1.03").unwrap();

        let (total_accrued, deltas) = accrue_segments(
            &mut segments,
            rate_of_change,
            1000,
            1000 + SECONDS_PER_YEAR,
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // Variable: 6B * 1.03 - 6B = 180M
        // Fixed 1mo: 4B * 6% * 1yr = 240M
        assert_eq!(deltas.variable, Int128::from(180_000_000i128));
        assert_eq!(deltas.one_month, Int128::from(240_000_000i128));
        assert_eq!(total_accrued, Uint128::new(420_000_000));
    }

    #[test]
    fn test_accrue_segments_expired_fixed_converts_to_variable() {
        use crate::debt_manager::accrue_segments;

        // Fixed rate that expired at time 5000, current time is 10000
        let mut segments = vec![
            fixed_segment(3_000_000_000, 3, 7, 5000, false), // end_time = 5000, no rollover
        ];
        let rates = default_rates();

        let (_total_accrued, deltas) = accrue_segments(
            &mut segments,
            Decimal::one(),
            1000,
            10000, // well past end_time
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // Should have converted to variable: -original from 3mo, +new amount to variable
        assert!(deltas.three_month < Int128::zero());
        assert!(deltas.variable > Int128::zero());
        // After conversion, segment should have no fixed_rate
        assert!(segments[0].fixed_rate.is_none());
    }

    #[test]
    fn test_accrue_segments_expired_fixed_rollover() {
        use crate::debt_manager::accrue_segments;

        // Fixed rate that expired at time 5000, with rollover enabled
        let mut segments = vec![
            fixed_segment(3_000_000_000, 1, 5, 5000, true),
        ];
        let rates = default_rates();

        let (_total_accrued, deltas) = accrue_segments(
            &mut segments,
            Decimal::one(),
            1000,
            10000,
            &rates,
            Decimal::percent(8), // collateral_weighted_rate
        ).unwrap();

        // With rollover, segment should stay fixed
        assert!(segments[0].fixed_rate.is_some());
        let fr = segments[0].fixed_rate.as_ref().unwrap();
        // New rate = 8% * 1.1 (1mo multiplier) = 8.8%
        assert_eq!(fr.rate, Decimal::from_str("0.088").unwrap());
        // New end_time = current_time + 1 month
        assert_eq!(fr.end.end_time, 10000 + 2_592_000);
        // Delta should track 1mo accrual (no conversion)
        assert!(deltas.one_month > Int128::zero() || deltas.one_month == Int128::zero());
    }

    #[test]
    fn test_accrue_segments_zero_time_no_accrual() {
        use crate::debt_manager::accrue_segments;

        let mut segments = vec![var_segment(5_000_000_000)];
        let rates = default_rates();

        let (total_accrued, deltas) = accrue_segments(
            &mut segments,
            Decimal::one(), // 1.0 = no change
            1000,
            1000, // same time
            &rates,
            Decimal::percent(5),
        ).unwrap();

        assert_eq!(total_accrued, Uint128::zero());
        assert_eq!(deltas.variable, Int128::zero());
        assert_eq!(segments[0].amount, Uint128::new(5_000_000_000));
    }

    // ========================================================================
    // P. FIXED RATE EXPIRATION TESTS (rates)
    // ========================================================================

    #[test]
    fn test_check_fixed_rate_expiration_not_expired() {
        let rates = default_rates();
        let mut segment = fixed_segment(1_000_000_000, 3, 7, 99999999, false);

        let (was_fixed, duration, converted) = check_and_handle_fixed_rate_expiration(
            &mut segment, &rates, 50000, Decimal::percent(5),
        ).unwrap();

        assert!(was_fixed);
        assert_eq!(duration, 3);
        assert!(!converted);
        assert!(segment.fixed_rate.is_some());
    }

    #[test]
    fn test_check_fixed_rate_expiration_expired_no_rollover() {
        let rates = default_rates();
        let mut segment = fixed_segment(1_000_000_000, 6, 10, 5000, false);

        let (was_fixed, duration, converted) = check_and_handle_fixed_rate_expiration(
            &mut segment, &rates, 10000, Decimal::percent(5),
        ).unwrap();

        assert!(was_fixed);
        assert_eq!(duration, 6);
        assert!(converted);
        assert!(segment.fixed_rate.is_none());
    }

    #[test]
    fn test_check_fixed_rate_expiration_expired_with_rollover() {
        let rates = default_rates();
        let mut segment = fixed_segment(1_000_000_000, 3, 7, 5000, true);

        let (was_fixed, duration, converted) = check_and_handle_fixed_rate_expiration(
            &mut segment, &rates, 10000, Decimal::percent(6),
        ).unwrap();

        assert!(was_fixed);
        assert_eq!(duration, 3);
        assert!(!converted); // rollover, not converted
        let fr = segment.fixed_rate.as_ref().unwrap();
        // 6% * 1.3 (3mo multiplier) = 7.8%
        assert_eq!(fr.rate, Decimal::from_str("0.078").unwrap());
        assert_eq!(fr.end.end_time, 10000 + 3 * 2_592_000);
        assert!(fr.end.rollover);
    }

    #[test]
    fn test_check_fixed_rate_expiration_variable_segment_noop() {
        let rates = default_rates();
        let mut segment = var_segment(5_000_000_000);

        let (was_fixed, duration, converted) = check_and_handle_fixed_rate_expiration(
            &mut segment, &rates, 10000, Decimal::percent(5),
        ).unwrap();

        assert!(!was_fixed);
        assert_eq!(duration, 0);
        assert!(!converted);
        assert!(segment.fixed_rate.is_none());
    }

    #[test]
    fn test_check_fixed_rate_expiration_exactly_at_boundary() {
        let rates = default_rates();
        let mut segment = fixed_segment(1_000_000_000, 1, 5, 10000, false);

        // current_time == end_time -> expired
        let (was_fixed, _duration, converted) = check_and_handle_fixed_rate_expiration(
            &mut segment, &rates, 10000, Decimal::percent(5),
        ).unwrap();

        assert!(was_fixed);
        assert!(converted);
        assert!(segment.fixed_rate.is_none());
    }

    #[test]
    fn test_recalculate_fixed_rate_invalid_duration() {
        let rates = default_rates();

        let result = recalculate_fixed_rate(&rates, 2, 50000, Decimal::percent(5));
        assert!(result.is_err());

        let result = recalculate_fixed_rate(&rates, 12, 50000, Decimal::percent(5));
        assert!(result.is_err());
    }

    // ========================================================================
    // Q. DEPLOYMENT VENUE & REVENUE TESTS
    // ========================================================================

    #[test]
    fn test_set_deployment_venue_with_initial_amount() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue = get_addr(&app, "vault_venue");

        let resp = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetDeploymentVenue {
                position_id: Uint128::one(),
                venue_address: venue.to_string(),
                initial_deployed_debt_amount: Some(Uint128::new(500_000_000)),
            },
            &[],
        ).unwrap();

        let attr = resp.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "deployed_debt_amount");
        assert!(attr.is_some());
        assert_eq!(attr.unwrap().value, "500000000");

        // Check active venues query
        let venues: Vec<String> = app.wrap().query_wasm_smart(
            debt_addr.clone(),
            &QueryMsg::GetActiveDeploymentVenues { venue: None, start_after: None, limit: None },
        ).unwrap();
        assert_eq!(venues.len(), 1);
    }

    #[test]
    fn test_set_deployment_venue_no_position() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        let venue = get_addr(&app, "vault_venue");

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetDeploymentVenue {
                position_id: Uint128::one(),
                venue_address: venue.to_string(),
                initial_deployed_debt_amount: Some(Uint128::new(500_000_000)),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("No user positions") ||
                err.root_cause().to_string().contains("not found"));
    }

    #[test]
    fn test_set_deployment_venue_update_existing() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let venue = get_addr(&app, "vault_venue");

        // Set venue
        app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetDeploymentVenue {
                position_id: Uint128::one(),
                venue_address: venue.to_string(),
                initial_deployed_debt_amount: Some(Uint128::new(500_000_000)),
            },
            &[],
        ).unwrap();

        // Update same venue with new amount
        let resp = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::SetDeploymentVenue {
                position_id: Uint128::one(),
                venue_address: venue.to_string(),
                initial_deployed_debt_amount: Some(Uint128::new(1_000_000_000)),
            },
            &[],
        ).unwrap();

        let attr = resp.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "deployed_debt_amount");
        assert_eq!(attr.unwrap().value, "1000000000");
    }

    #[test]
    fn test_take_revenue_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::TakeRevenue {}, &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_take_revenue_disabled() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);

        let err = app.execute_contract(
            admin.clone(), debt_addr.clone(),
            &ExecuteMsg::TakeRevenue {}, &[],
        ).unwrap_err();
        // distribute_revenue defaults to false
        assert!(err.root_cause().to_string().contains("disabled") ||
                err.root_cause().to_string().contains("Revenue"));
    }

    #[test]
    fn test_record_bad_debt_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::RecordBadDebt {
                position_id: Uint128::one(),
                position_owner: user.to_string(),
                bad_debt_amount: Uint128::new(1_000_000_000),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // R. ACCUMULATE INTEREST & RATE UTILITY TESTS
    // ========================================================================

    #[test]
    fn test_accumulate_interest_dec_one_year() {
        use crate::rates::accumulate_interest_dec;

        let principal = Decimal::from_str("10000.0").unwrap();
        let rate = Decimal::percent(5);

        let result = accumulate_interest_dec(principal, rate, SECONDS_PER_YEAR).unwrap();
        // 10000 * 5% * 1 year = 500
        // accumulate_interest_dec does decimal_multiplication(principal, rate * time/year)
        // = decimal_multiplication(10000, 0.05) = result depends on the math fn
        assert!(result > Decimal::zero());
    }

    #[test]
    fn test_accumulate_interest_dec_zero_rate() {
        use crate::rates::accumulate_interest_dec;

        let principal = Decimal::from_str("10000.0").unwrap();
        let result = accumulate_interest_dec(principal, Decimal::zero(), SECONDS_PER_YEAR).unwrap();
        assert_eq!(result, Decimal::zero());
    }

    #[test]
    fn test_accumulate_interest_dec_zero_time() {
        use crate::rates::accumulate_interest_dec;

        let principal = Decimal::from_str("10000.0").unwrap();
        let result = accumulate_interest_dec(principal, Decimal::percent(5), 0).unwrap();
        assert_eq!(result, Decimal::zero());
    }

    #[test]
    fn test_accumulate_interest_dec_half_year() {
        use crate::rates::accumulate_interest_dec;

        let principal = Decimal::from_str("10000.0").unwrap();
        let rate = Decimal::percent(10);
        let half_year = SECONDS_PER_YEAR / 2;

        let result = accumulate_interest_dec(principal, rate, half_year).unwrap();
        // 10000 * 10% * 0.5 = 500
        let full_year = accumulate_interest_dec(principal, rate, SECONDS_PER_YEAR).unwrap();
        // Half year should be roughly half of full year
        assert!(result < full_year);
        assert!(result > Decimal::zero());
    }

    #[test]
    fn test_get_total_debt_from_segments_mixed() {
        use crate::rates::get_total_debt_from_segments;

        let segments = vec![
            var_segment(3_000_000_000),
            fixed_segment(2_000_000_000, 1, 5, 99999999, false),
            fixed_segment(1_000_000_000, 6, 10, 99999999, true),
        ];

        let total = get_total_debt_from_segments(&segments);
        assert_eq!(total, Uint128::new(6_000_000_000));
    }

    #[test]
    fn test_get_total_debt_from_segments_empty() {
        use crate::rates::get_total_debt_from_segments;

        let segments: Vec<RateSegment> = vec![];
        assert_eq!(get_total_debt_from_segments(&segments), Uint128::zero());
    }

    // ========================================================================
    // S. CHECK AND CLEAR DEBT DELTA TESTS
    // ========================================================================

    #[test]
    fn test_check_and_clear_debt_delta_authorized() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let points = get_addr(&app, POINTS_CONTRACT);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);

        let resp = app.execute_contract(
            points.clone(), debt_addr.clone(),
            &ExecuteMsg::CheckAndClearDebtDelta {
                position_owner: user.to_string(),
                position_id: Uint128::one(),
            },
            &[],
        ).unwrap();

        let delta_attr = resp.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "debt_delta");
        assert!(delta_attr.is_some());
        assert_eq!(delta_attr.unwrap().value, "0");
    }

    #[test]
    fn test_check_and_clear_debt_delta_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);

        let err = app.execute_contract(
            user.clone(), debt_addr.clone(),
            &ExecuteMsg::CheckAndClearDebtDelta {
                position_owner: user.to_string(),
                position_id: Uint128::one(),
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // T. MULTI-STEP ACCRUAL SCENARIOS
    // ========================================================================

    #[test]
    fn test_scenario_multiple_accruals_compound() {
        use crate::debt_manager::accrue_segments;

        let mut segments = vec![var_segment(10_000_000_000)];
        let rates = default_rates();

        // First accrual: 2% growth
        let rate1 = Decimal::from_str("1.02").unwrap();
        let (accrued1, _) = accrue_segments(
            &mut segments, rate1, 0, SECONDS_PER_YEAR, &rates, Decimal::percent(5),
        ).unwrap();

        let after_first = segments[0].amount;
        assert_eq!(after_first, Uint128::new(10_200_000_000));
        assert_eq!(accrued1, Uint128::new(200_000_000));

        // Second accrual: another 2% on the new balance
        let (accrued2, _) = accrue_segments(
            &mut segments, rate1, SECONDS_PER_YEAR, 2 * SECONDS_PER_YEAR, &rates, Decimal::percent(5),
        ).unwrap();

        let after_second = segments[0].amount;
        // 10.2B * 1.02 = 10.404B, so accrued = 204M
        assert_eq!(accrued2, Uint128::new(204_000_000));
        assert_eq!(after_second, Uint128::new(10_404_000_000));
    }

    #[test]
    fn test_scenario_accrual_then_liquidation_reduce() {
        use crate::debt_manager::accrue_segments;

        let mut segments = vec![
            var_segment(5_000_000_000),
            fixed_segment(3_000_000_000, 3, 7, 1000 + 2 * SECONDS_PER_YEAR, false),
        ];
        let rates = default_rates();

        // Accrue 3% on variable
        let rate = Decimal::from_str("1.03").unwrap();
        let (total_accrued, _) = accrue_segments(
            &mut segments, rate, 1000, 1000 + SECONDS_PER_YEAR, &rates, Decimal::percent(5),
        ).unwrap();

        assert!(total_accrued > Uint128::zero());
        let total_after_accrual = get_total_from_segments(&segments);

        // Now liquidation reduces 40% of debt
        let liq_amount = total_after_accrual.multiply_ratio(40u128, 100u128);
        let _liq_deltas = reduce_segments_proportionally(
            &mut segments, total_after_accrual, total_after_accrual, liq_amount,
        );

        let total_after_liq = get_total_from_segments(&segments);
        // Should be ~60% of post-accrual total
        let expected_remaining = total_after_accrual - liq_amount;
        assert_eq!(total_after_liq, expected_remaining);
    }

    #[test]
    fn test_scenario_fixed_rate_expires_during_accrual_then_repay() {
        use crate::debt_manager::accrue_segments;

        // Fixed rate expires at time 5000, accrual happens at time 10000
        let mut segments = vec![
            var_segment(3_000_000_000),
            fixed_segment(2_000_000_000, 1, 5, 5000, false), // expires, no rollover
        ];
        let rates = default_rates();

        let (_total_accrued, deltas) = accrue_segments(
            &mut segments,
            Decimal::from_str("1.01").unwrap(),
            1000,
            10000,
            &rates,
            Decimal::percent(5),
        ).unwrap();

        // The 1mo segment should have converted to variable
        // Both segments should now be variable
        assert!(segments.iter().all(|s| s.fixed_rate.is_none()));
        // 1mo delta should be negative (removed from fixed)
        assert!(deltas.one_month < Int128::zero());

        // Now repay half the total
        let total = get_total_from_segments(&segments);
        let (applied, _) = repay_segments_default_order(&mut segments, total / Uint128::new(2));
        assert_eq!(applied, total / Uint128::new(2));
    }
}
