#![allow(non_snake_case)]
//! Rate engine accrual tests for the Debt contract.
//!
//! These tests exercise `accrue_position`, `update_rate_indices`,
//! `get_interest_rates`, and the external `accrue` entry point using
//! `cosmwasm_std::testing::mock_dependencies()` for direct storage access.
//!
//! With `skip_credit_price_accrual: true` in Config, step 1 (credit price
//! controller) of accrue_position is bypassed, avoiding oracle queries.

#[cfg(test)]
pub mod tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{Addr, Decimal, Env, OwnedDeps, Timestamp, Uint128};
    use cosmwasm_std::MemoryStorage;
    use cosmwasm_std::testing::MockApi;
    use cosmwasm_std::testing::MockQuerier;
    use std::str::FromStr;

    use membrane::debt::{AssetRateIndex, BasketDebt, Config, DebtPosition};
    use membrane::oracle::PriceResponse;
    use membrane::types::{
        Asset, AssetInfo, CreditAssetBreakdown, FixedRate, FixedRateCap, FixedRateCaps,
        FixedRateEnd, IRMConfig, PendingRevenue, Rate, Rates, RateSegment,
    };

    use crate::rates::{
        accrue, accrue_position, get_interest_rates, get_total_position_debt, update_rate_indices,
        CollateralTypeInfo, COLLATERAL_TYPES_CACHE, SECONDS_PER_YEAR,
    };
    use crate::state::{
        BasketRateIndex, BASKET, BASKET_RATE_INDICES, CDT_SUPPLY, CONFIG, FROZEN, POSITIONS, RATES,
    };

    // ========================================================================
    // Constants
    // ========================================================================

    const CDT_DENOM: &str = "factory/creator/ucdt";
    const ATOM_DENOM: &str = "uatom";
    const OSMO_DENOM: &str = "uosmo";

    // ========================================================================
    // Helper: default IRM config
    // ========================================================================

    fn default_irm_config() -> IRMConfig {
        IRMConfig {
            adjustment_speed: Decimal::from_ratio(50u128, 1u128),
            min_adaptive_rate: Decimal::permille(1), // 0.1%
            max_adaptive_rate: Decimal::from_ratio(2u128, 1u128), // 200%
        }
    }

    // ========================================================================
    // Helper: default Rates
    // ========================================================================

    fn default_rates(block_time: u64) -> Rates {
        Rates {
            current_adaptive_rate: vec![Decimal::percent(5)],
            peg_current_adaptive_rate: vec![Decimal::percent(5)],
            lastest_collateral_rates: vec![Rate {
                rate: Decimal::percent(5),
                last_time_updated: block_time,
            }],
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: default_fixed_rate_caps(),
            rates_last_accrued: block_time,
            credit_last_accrued: block_time,
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        }
    }

    fn default_fixed_rate_caps() -> FixedRateCaps {
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

    // ========================================================================
    // Helper: default Config (skip_credit_price_accrual = true)
    // ========================================================================

    fn default_config() -> Config {
        Config {
            owner: Addr::unchecked("admin"),
            collateral_contract: None,
            liquidation_contract: None,
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None, // None bypasses oracle queries
            liquidity_contract: None,
            discounts_contract: None,
            revenue_distributor: None,
            transmuter_addr: None,
            points_contract: None,
            liq_fee: Decimal::percent(1),
            credit_twap_timeframe: 60,
            cpc_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(100_000),
            base_debt_cap_multiplier: Uint128::new(10),
            rate_slope_multiplier: Decimal::from_str("2").unwrap(),
            affiliate_fee_max: Decimal::percent(5),
            skip_credit_price_accrual: true,
            liquidation_stat_limit: 100,
            irm_config: default_irm_config(),
            acquisition_contract: None,
        }
    }

    // ========================================================================
    // Helper: default BasketDebt
    // ========================================================================

    fn default_basket() -> BasketDebt {
        BasketDebt {
            basket_id: Uint128::one(),
            credit_asset: CreditAssetBreakdown {
                info: AssetInfo::NativeToken {
                    denom: CDT_DENOM.to_string(),
                },
                variable_amount: Uint128::new(1_000_000),
                one_month_amount: Uint128::zero(),
                three_month_amount: Uint128::zero(),
                six_month_amount: Uint128::zero(),
                peg_variable_amount: Uint128::zero(),
                peg_one_month_amount: Uint128::zero(),
                peg_three_month_amount: Uint128::zero(),
                peg_six_month_amount: Uint128::zero(),
            },
            // IMPORTANT: Set to zero so accrue_position's CPC section sees
            // credit_TWAP_price(0) == credit_price(0) => Ordering::Equal => no division.
            // oracle_contract=None already sets credit_TWAP_price=0 and skip=true,
            // but the price_difference calc runs unconditionally and would divide
            // by credit_TWAP_price if prices differ.
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::zero(),
                decimals: 6,
            },
            pending_revenue: PendingRevenue {
                total_pending: Uint128::zero(),
                per_asset_rev: vec![],
            },
            pending_bad_debt: Uint128::zero(),
            liq_queue: None,
            oracle_set: false,
            distribute_revenue: false,
        }
    }

    // ========================================================================
    // Helper: init_test_state
    //
    // Initializes RATES, BASKET, CONFIG, CDT_SUPPLY, FROZEN,
    // BASKET_RATE_INDICES, and COLLATERAL_TYPES_CACHE with reasonable defaults.
    // ========================================================================

    fn init_test_state(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) {
        let block_time = 1_000_000u64;

        CONFIG
            .save(deps.as_mut().storage, &default_config())
            .unwrap();
        BASKET
            .save(deps.as_mut().storage, &default_basket())
            .unwrap();
        RATES
            .save(deps.as_mut().storage, &default_rates(block_time))
            .unwrap();
        CDT_SUPPLY
            .save(deps.as_mut().storage, &Uint128::new(10_000_000))
            .unwrap();
        FROZEN.save(deps.as_mut().storage, &false).unwrap();

        // Single collateral type: ATOM
        let collateral_types = vec![CollateralTypeInfo {
            asset_denom: ATOM_DENOM.to_string(),
            max_ltv: Decimal::percent(80), // 0.80
        }];
        COLLATERAL_TYPES_CACHE
            .save(deps.as_mut().storage, &collateral_types)
            .unwrap();

        // Basket rate index starts at 1.0
        BASKET_RATE_INDICES
            .save(
                deps.as_mut().storage,
                ATOM_DENOM.to_string(),
                &BasketRateIndex {
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                },
            )
            .unwrap();
    }

    /// Variant with two collateral types
    fn init_test_state_two_assets(
        deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    ) {
        init_test_state(deps);

        // Add second collateral type
        let collateral_types = vec![
            CollateralTypeInfo {
                asset_denom: ATOM_DENOM.to_string(),
                max_ltv: Decimal::percent(80),
            },
            CollateralTypeInfo {
                asset_denom: OSMO_DENOM.to_string(),
                max_ltv: Decimal::percent(60),
            },
        ];
        COLLATERAL_TYPES_CACHE
            .save(deps.as_mut().storage, &collateral_types)
            .unwrap();

        // Update RATES to have two-element adaptive rate vectors
        let block_time = 1_000_000u64;
        let mut rates = default_rates(block_time);
        rates.current_adaptive_rate = vec![Decimal::percent(5), Decimal::percent(5)];
        rates.peg_current_adaptive_rate = vec![Decimal::percent(5), Decimal::percent(5)];
        rates.lastest_collateral_rates = vec![
            Rate {
                rate: Decimal::percent(5),
                last_time_updated: block_time,
            },
            Rate {
                rate: Decimal::percent(5),
                last_time_updated: block_time,
            },
        ];
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        // Basket rate index for second asset
        BASKET_RATE_INDICES
            .save(
                deps.as_mut().storage,
                OSMO_DENOM.to_string(),
                &BasketRateIndex {
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                },
            )
            .unwrap();
    }

    // ========================================================================
    // Helper: create_test_position
    // ========================================================================

    fn create_test_position(var_amount: u128) -> DebtPosition {
        DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(var_amount),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![AssetRateIndex {
                asset_denom: ATOM_DENOM.to_string(),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                ratio: Decimal::one(), // 100% in ATOM
            }],
        }
    }

    fn create_test_position_empty_indices(var_amount: u128) -> DebtPosition {
        DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(var_amount),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![], // Empty: should be bootstrapped
        }
    }

    fn create_fixed_rate_position(
        amount: u128,
        rate: Decimal,
        end_time: u64,
        rollover: bool,
        duration_months: u8,
    ) -> DebtPosition {
        DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(amount),
                fixed_rate: Some(FixedRate {
                    rate,
                    end: FixedRateEnd {
                        end_time,
                        rollover,
                        duration_months,
                    },
                }),
            }],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![AssetRateIndex {
                asset_denom: ATOM_DENOM.to_string(),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                ratio: Decimal::one(),
            }],
        }
    }

    // ========================================================================
    // Helper: create mock env at a specific time
    // ========================================================================

    fn env_at_time(seconds: u64) -> Env {
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(seconds);
        env
    }

    // ========================================================================
    // Helper: call accrue_position with proper borrow splitting.
    //
    // The functions take (&mut dyn Storage, QuerierWrapper, ...) but we cannot
    // simultaneously call deps.as_mut().storage and deps.as_ref().querier.
    // Instead we use deps.as_mut() once to get a DepsMut, then use its fields.
    // ========================================================================

    fn call_accrue_position(
        deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
        env: Env,
        config: &Config,
        position: &mut DebtPosition,
        basket: &mut BasketDebt,
        user: &str,
    ) -> cosmwasm_std::StdResult<()> {
        let deps_mut = deps.as_mut();
        accrue_position(
            deps_mut.storage,
            deps_mut.querier,
            env,
            config,
            position,
            basket,
            user.to_string(),
        )
    }

    fn call_update_rate_indices(
        deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
        env: Env,
        config: &Config,
    ) -> cosmwasm_std::StdResult<()> {
        let deps_mut = deps.as_mut();
        update_rate_indices(deps_mut.storage, deps_mut.querier, env, config)
    }

    fn call_get_interest_rates(
        deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
        env: Env,
        collateral_types: &[CollateralTypeInfo],
        rates: &Rates,
        config: &Config,
        tvl_ratios: Option<Vec<Decimal>>,
    ) -> cosmwasm_std::StdResult<Vec<Decimal>> {
        let deps_mut = deps.as_mut();
        get_interest_rates(
            deps_mut.storage,
            deps_mut.querier,
            env,
            collateral_types,
            rates,
            config,
            tvl_ratios,
        )
    }

    // ========================================================================
    // GROUP A: accrue_position() tests
    // ========================================================================

    #[test]
    fn test_accrue_position_variable_debt_grows_after_time() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        let initial_debt = get_total_position_debt(&position);
        assert_eq!(initial_debt, Uint128::new(1_000_000));

        // Advance 1 year from block_time = 1_000_000
        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        let new_debt = get_total_position_debt(&position);
        // With 5% adaptive rate over 1 year, debt should grow
        assert!(
            new_debt > initial_debt,
            "Debt should have grown: {} vs {}",
            new_debt,
            initial_debt
        );
    }

    #[test]
    fn test_accrue_position_fixed_rate_segment_accrues() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        // Fixed rate at 10%, expires far in the future
        let mut position = create_fixed_rate_position(
            1_000_000,
            Decimal::percent(10),
            1_000_000 + 10 * SECONDS_PER_YEAR, // expires far out
            false,
            3,
        );

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        let new_debt = get_total_position_debt(&position);
        // 1_000_000 at 10% for 1 year = 100_000 accrued -> 1_100_000
        assert_eq!(
            new_debt,
            Uint128::new(1_100_000),
            "Fixed rate accrual: expected 1_100_000, got {}",
            new_debt
        );
    }

    #[test]
    fn test_accrue_position_peg_segments_accrue_independently() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();

        let mut position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(500_000),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![RateSegment {
                amount: Uint128::new(500_000),
                fixed_rate: None,
            }],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![AssetRateIndex {
                asset_denom: ATOM_DENOM.to_string(),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                ratio: Decimal::one(),
            }],
        };

        let total_before = get_total_position_debt(&position);
        assert_eq!(total_before, Uint128::new(1_000_000));

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        let total_after = get_total_position_debt(&position);
        assert!(total_after > total_before);

        // Both segments should have grown
        let regular_debt: Uint128 = position.rate_segments.iter().map(|s| s.amount).sum();
        let peg_debt: Uint128 = position.peg_rate_segments.iter().map(|s| s.amount).sum();
        assert!(regular_debt > Uint128::new(500_000));
        assert!(peg_debt > Uint128::new(500_000));
    }

    #[test]
    fn test_accrue_position_syncs_collateral_rate_indices_to_basket() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        // Position starts at rate_index=1.0
        assert_eq!(
            position.collateral_rate_indices[0].rate_index,
            Decimal::one()
        );

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // After accrual, position rate index should equal the basket rate index
        let basket_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();

        assert_eq!(
            position.collateral_rate_indices[0].rate_index,
            basket_idx.rate_index,
            "Position rate_index should sync to basket"
        );
        assert_eq!(
            position.collateral_rate_indices[0].peg_rate_index,
            basket_idx.peg_rate_index,
            "Position peg_rate_index should sync to basket"
        );
        // Basket rate index should be > 1 after accrual
        assert!(basket_idx.rate_index > Decimal::one());
    }

    #[test]
    fn test_accrue_position_bootstraps_empty_collateral_rate_indices() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position_empty_indices(1_000_000);

        assert!(position.collateral_rate_indices.is_empty());

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // After accrual, collateral_rate_indices should be populated
        assert_eq!(
            position.collateral_rate_indices.len(),
            1,
            "Should have bootstrapped 1 collateral rate index"
        );
        assert_eq!(
            position.collateral_rate_indices[0].asset_denom,
            ATOM_DENOM
        );
    }

    #[test]
    fn test_accrue_position_zero_debt_no_revenue() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(0); // zero debt

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // No revenue should be added
        assert_eq!(basket.pending_revenue.total_pending, Uint128::zero());
        assert_eq!(position.pending_interest, Uint128::zero());
    }

    #[test]
    fn test_accrue_position_revenue_distributed_to_pending() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // Revenue should have been added
        assert!(
            basket.pending_revenue.total_pending > Uint128::zero(),
            "pending_revenue.total_pending should be > 0"
        );
        assert!(
            position.pending_interest > Uint128::zero(),
            "position.pending_interest should be > 0"
        );
        assert!(
            position.total_interest_accrued > Uint128::zero(),
            "position.total_interest_accrued should be > 0"
        );
        // pending_revenue should equal position.pending_interest
        assert_eq!(
            basket.pending_revenue.total_pending,
            position.pending_interest
        );
    }

    #[test]
    fn test_accrue_position_basket_credit_asset_breakdown_updated() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let initial_variable = basket.credit_asset.variable_amount;

        let mut position = create_test_position(1_000_000);

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // Variable amount in basket should increase by accrued interest
        assert!(
            basket.credit_asset.variable_amount > initial_variable,
            "basket credit_asset variable_amount should increase: {} vs {}",
            basket.credit_asset.variable_amount,
            initial_variable
        );
    }

    #[test]
    fn test_accrue_position_per_asset_revenue_tracking() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // Per-asset revenue should have an entry for ATOM
        assert_eq!(basket.pending_revenue.per_asset_rev.len(), 1);
        let atom_rev = &basket.pending_revenue.per_asset_rev[0];
        assert!(atom_rev.info.equal(&AssetInfo::NativeToken {
            denom: ATOM_DENOM.to_string()
        }));
        assert!(atom_rev.amount > Uint128::zero());
    }

    #[test]
    fn test_accrue_position_fixed_rate_expired_no_rollover_converts_to_variable() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        // Fixed rate expires at time 1_500_000 (before our accrual time)
        let mut position = create_fixed_rate_position(
            1_000_000,
            Decimal::percent(10),
            1_500_000, // expires before accrual
            false,     // no rollover
            1,
        );

        // Accrual at time 2_000_000 (after expiration)
        let env = env_at_time(2_000_000);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // The segment should now be variable (fixed_rate = None)
        assert!(
            position.rate_segments[0].fixed_rate.is_none(),
            "Expired non-rollover fixed rate should convert to variable"
        );
        // Debt should have accrued with the fixed rate for the period
        assert!(position.rate_segments[0].amount > Uint128::new(1_000_000));
    }

    #[test]
    fn test_accrue_position_fixed_rate_expired_with_rollover() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        // Fixed rate expires at time 1_500_000 with rollover
        let mut position = create_fixed_rate_position(
            1_000_000,
            Decimal::percent(10),
            1_500_000, // expires before accrual
            true,      // rollover enabled
            3,         // 3-month duration
        );

        // Accrual at time 2_000_000 (after expiration)
        let env = env_at_time(2_000_000);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // The segment should still be fixed (rolled over)
        assert!(
            position.rate_segments[0].fixed_rate.is_some(),
            "Rollover fixed rate should stay fixed"
        );

        let fr = position.rate_segments[0].fixed_rate.as_ref().unwrap();
        // New end_time should be current_time + 3 * 30 days
        let expected_end = 2_000_000 + 3 * 2_592_000;
        assert_eq!(fr.end.end_time, expected_end);
        assert!(fr.end.rollover, "Rollover flag should remain true");
    }

    #[test]
    fn test_accrue_position_multiple_accruals_compound() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        // First accrual: half year
        let env1 = env_at_time(1_000_000 + SECONDS_PER_YEAR / 2);
        call_accrue_position(&mut deps, env1, &config, &mut position, &mut basket, "user")
            .unwrap();

        let debt_after_first = get_total_position_debt(&position);
        assert!(debt_after_first > Uint128::new(1_000_000));

        // Second accrual: another half year (total 1 year)
        let env2 = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        call_accrue_position(&mut deps, env2, &config, &mut position, &mut basket, "user")
            .unwrap();

        let debt_after_second = get_total_position_debt(&position);
        assert!(
            debt_after_second > debt_after_first,
            "Second accrual should further increase debt: {} vs {}",
            debt_after_second,
            debt_after_first
        );
    }

    #[test]
    fn test_accrue_position_no_time_elapsed_no_growth() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        // Accrual at exactly the same time as rates_last_accrued
        let env = env_at_time(1_000_000);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        let debt_after = get_total_position_debt(&position);
        // With zero elapsed time, rate indices don't change, so rate_of_change = 1.0
        // (basket_rate_index / position_rate_index = 1.0/1.0)
        // Variable accrual: 1_000_000 * 1.0 = 1_000_000 (no growth)
        assert_eq!(debt_after, Uint128::new(1_000_000));
        assert_eq!(basket.pending_revenue.total_pending, Uint128::zero());
    }

    #[test]
    fn test_accrue_position_mixed_variable_and_fixed_segments() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();

        let mut position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![
                RateSegment {
                    amount: Uint128::new(500_000),
                    fixed_rate: None, // variable
                },
                RateSegment {
                    amount: Uint128::new(500_000),
                    fixed_rate: Some(FixedRate {
                        rate: Decimal::percent(8),
                        end: FixedRateEnd {
                            end_time: 1_000_000 + 10 * SECONDS_PER_YEAR,
                            rollover: false,
                            duration_months: 6,
                        },
                    }),
                },
            ],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![AssetRateIndex {
                asset_denom: ATOM_DENOM.to_string(),
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                ratio: Decimal::one(),
            }],
        };

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // Variable segment should have grown
        assert!(position.rate_segments[0].amount > Uint128::new(500_000));

        // Fixed segment: 500_000 at 8% for 1 year = 40_000 accrued
        assert_eq!(
            position.rate_segments[1].amount,
            Uint128::new(540_000),
            "Fixed at 8% for 1yr: 500k + 40k = 540k"
        );

        // Total interest should match
        let total_debt = get_total_position_debt(&position);
        assert!(total_debt > Uint128::new(1_000_000));
    }

    // ========================================================================
    // GROUP B: update_rate_indices() tests
    // ========================================================================

    #[test]
    fn test_update_rate_indices_empty_collateral_cache_updates_timestamp() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Clear collateral types cache
        COLLATERAL_TYPES_CACHE
            .save(deps.as_mut().storage, &vec![])
            .unwrap();

        let config = default_config();
        let new_time = 2_000_000u64;
        let env = env_at_time(new_time);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let rates = RATES.load(deps.as_ref().storage).unwrap();
        assert_eq!(
            rates.rates_last_accrued, new_time,
            "rates_last_accrued should update even with empty cache"
        );
    }

    #[test]
    fn test_update_rate_indices_single_asset_grows_index() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();

        let initial_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        assert_eq!(initial_idx.rate_index, Decimal::one());

        // Advance 1 year
        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let updated_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();

        assert!(
            updated_idx.rate_index > Decimal::one(),
            "Basket rate index should grow after update: {}",
            updated_idx.rate_index
        );
        assert!(
            updated_idx.peg_rate_index > Decimal::one(),
            "Peg rate index should also grow: {}",
            updated_idx.peg_rate_index
        );
    }

    #[test]
    fn test_update_rate_indices_bootstraps_adaptive_rate_vectors() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Set adaptive rate vectors to empty
        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.current_adaptive_rate = vec![];
        rates.peg_current_adaptive_rate = vec![];
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let config = default_config();
        let env = env_at_time(1_000_000 + 86400);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let rates_after = RATES.load(deps.as_ref().storage).unwrap();
        assert_eq!(
            rates_after.current_adaptive_rate.len(),
            1,
            "Should bootstrap current_adaptive_rate to match collateral_types"
        );
        assert_eq!(
            rates_after.peg_current_adaptive_rate.len(),
            1,
            "Should bootstrap peg_current_adaptive_rate to match collateral_types"
        );
        // Bootstrapped from zero should jump to destination (adaptive_smooth_rate bootstrap logic)
        assert!(rates_after.current_adaptive_rate[0] > Decimal::zero());
    }

    #[test]
    fn test_update_rate_indices_historical_rates_saved() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let target_time = 1_000_000 + SECONDS_PER_YEAR;
        let env = env_at_time(target_time);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        // Check historical rates were saved
        let history = crate::state::HISTORICAL_INTEREST_RATES
            .may_load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        assert!(history.is_some(), "Historical rates should be saved");
        let history = history.unwrap();
        assert!(!history.is_empty(), "Should have at least one history entry");
        assert_eq!(
            history[0].timestamp, target_time,
            "History timestamp should match env time"
        );
    }

    #[test]
    fn test_update_rate_indices_rates_last_accrued_updated() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let new_time = 1_000_000 + 86400;
        let env = env_at_time(new_time);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let rates = RATES.load(deps.as_ref().storage).unwrap();
        assert_eq!(rates.rates_last_accrued, new_time);
    }

    #[test]
    fn test_update_rate_indices_peg_mirrors_regular() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let rates = RATES.load(deps.as_ref().storage).unwrap();
        // peg_current_adaptive_rate should mirror current_adaptive_rate
        assert_eq!(
            rates.current_adaptive_rate[0], rates.peg_current_adaptive_rate[0],
            "Peg rate should mirror regular rate"
        );
    }

    // ========================================================================
    // GROUP C: get_interest_rates() tests
    // ========================================================================

    #[test]
    fn test_get_interest_rates_no_oracle_ltv_fallback() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config(); // oracle_contract = None
        let collateral_types = vec![CollateralTypeInfo {
            asset_denom: ATOM_DENOM.to_string(),
            max_ltv: Decimal::percent(80),
        }];

        let rates_store = RATES.load(deps.as_ref().storage).unwrap();
        let env = env_at_time(1_000_000);

        let result =
            call_get_interest_rates(&mut deps, env, &collateral_types, &rates_store, &config, None)
                .unwrap();

        assert_eq!(result.len(), 1);
        // Fallback: base_rate * (1 / max_LTV) = 0.02 * (1/0.8) = 0.025
        let expected = Decimal::from_str("0.025").unwrap();
        assert_eq!(
            result[0], expected,
            "LTV fallback rate: 2% * (1/0.80) = 2.5%"
        );
    }

    #[test]
    fn test_get_interest_rates_multiple_assets_ltv_fallback() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config(); // oracle_contract = None
        let collateral_types = vec![
            CollateralTypeInfo {
                asset_denom: ATOM_DENOM.to_string(),
                max_ltv: Decimal::percent(80), // 1/0.8 = 1.25
            },
            CollateralTypeInfo {
                asset_denom: OSMO_DENOM.to_string(),
                max_ltv: Decimal::percent(60), // 1/0.6 = 1.667
            },
        ];

        let rates_store = RATES.load(deps.as_ref().storage).unwrap();
        let env = env_at_time(1_000_000);

        let result =
            call_get_interest_rates(&mut deps, env, &collateral_types, &rates_store, &config, None)
                .unwrap();

        assert_eq!(result.len(), 2);
        // ATOM: 0.02 * (1/0.8) = 0.025
        assert_eq!(result[0], Decimal::from_str("0.025").unwrap());
        // OSMO: 0.02 * (1/0.6) = 0.033333...
        assert!(result[1] > Decimal::percent(3));
        assert!(result[1] < Decimal::percent(4));
    }

    #[test]
    fn test_get_interest_rates_zero_ltv_returns_base_rate() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let collateral_types = vec![CollateralTypeInfo {
            asset_denom: ATOM_DENOM.to_string(),
            max_ltv: Decimal::zero(), // zero LTV
        }];

        let rates_store = RATES.load(deps.as_ref().storage).unwrap();
        let env = env_at_time(1_000_000);

        let result =
            call_get_interest_rates(&mut deps, env, &collateral_types, &rates_store, &config, None)
                .unwrap();

        // With zero max_ltv, fallback is just base_interest_rate
        assert_eq!(result[0], Decimal::percent(2));
    }

    #[test]
    fn test_get_interest_rates_empty_collateral_types() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let collateral_types: Vec<CollateralTypeInfo> = vec![];
        let rates_store = RATES.load(deps.as_ref().storage).unwrap();
        let env = env_at_time(1_000_000);

        let result =
            call_get_interest_rates(&mut deps, env, &collateral_types, &rates_store, &config, None)
                .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_get_interest_rates_equal_weight_default_when_no_tvl_ratios() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // With oracle_contract = None, the function returns LTV fallback rates.
        // When two assets have the same max_ltv, both get the same rate.
        let config = default_config(); // oracle_contract = None

        let collateral_types = vec![
            CollateralTypeInfo {
                asset_denom: ATOM_DENOM.to_string(),
                max_ltv: Decimal::percent(80),
            },
            CollateralTypeInfo {
                asset_denom: OSMO_DENOM.to_string(),
                max_ltv: Decimal::percent(80),
            },
        ];

        let rates_store = RATES.load(deps.as_ref().storage).unwrap();
        let env = env_at_time(1_000_000);

        let result =
            call_get_interest_rates(&mut deps, env, &collateral_types, &rates_store, &config, None)
                .unwrap();

        // Both assets have same max_ltv, so both should get the same rate
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], result[1]);
    }

    // ========================================================================
    // GROUP D: accrue() external entry point tests
    // ========================================================================

    #[test]
    fn test_accrue_single_position() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Use addr_make for bech32-valid addresses (required by deps.api.addr_validate)
        let user_addr = deps.api.addr_make("user");
        let user_str = user_addr.to_string();

        // Store a position
        let position = create_test_position(1_000_000);
        POSITIONS
            .save(deps.as_mut().storage, user_addr.clone(), &vec![position])
            .unwrap();

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        let info = mock_info(&user_str, &[]);

        let result = accrue(
            deps.as_mut(),
            env,
            info,
            None, // position_owner defaults to sender
            vec![Uint128::one()],
        );

        assert!(result.is_ok(), "Accrue should succeed: {:?}", result.err());

        // Verify position was updated
        let positions = POSITIONS.load(deps.as_ref().storage, user_addr).unwrap();
        let pos = &positions[0];
        let debt = get_total_position_debt(pos);
        assert!(debt > Uint128::new(1_000_000));

        // Verify basket was updated
        let basket = BASKET.load(deps.as_ref().storage).unwrap();
        assert!(basket.pending_revenue.total_pending > Uint128::zero());
    }

    #[test]
    fn test_accrue_nonexistent_position_errors() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let user_addr = deps.api.addr_make("user");
        let user_str = user_addr.to_string();

        // Store a position but query a different id
        let position = create_test_position(1_000_000);
        POSITIONS
            .save(deps.as_mut().storage, user_addr, &vec![position])
            .unwrap();

        let env = env_at_time(1_000_000 + 86400);
        let info = mock_info(&user_str, &[]);

        let result = accrue(
            deps.as_mut(),
            env,
            info,
            None,
            vec![Uint128::new(999)], // nonexistent ID
        );

        assert!(result.is_err(), "Should error for nonexistent position");
    }

    #[test]
    fn test_accrue_no_rates_initialized_errors() {
        let mut deps = mock_dependencies();

        let user_addr = deps.api.addr_make("user");
        let user_str = user_addr.to_string();

        // Only init minimal state, NO RATES saved
        CONFIG
            .save(deps.as_mut().storage, &default_config())
            .unwrap();
        BASKET
            .save(deps.as_mut().storage, &default_basket())
            .unwrap();
        FROZEN.save(deps.as_mut().storage, &false).unwrap();

        let position = create_test_position(1_000_000);
        POSITIONS
            .save(deps.as_mut().storage, user_addr, &vec![position])
            .unwrap();

        let env = env_at_time(2_000_000);
        let info = mock_info(&user_str, &[]);

        let result = accrue(
            deps.as_mut(),
            env,
            info,
            None,
            vec![Uint128::one()],
        );

        assert!(
            result.is_err(),
            "Should error when RATES not initialized"
        );
    }

    #[test]
    fn test_accrue_with_explicit_position_owner() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let alice_addr = deps.api.addr_make("alice");
        let alice_str = alice_addr.to_string();
        let anyone_addr = deps.api.addr_make("anyone");
        let anyone_str = anyone_addr.to_string();

        // Store a position for alice
        let position = create_test_position(1_000_000);
        POSITIONS
            .save(
                deps.as_mut().storage,
                alice_addr.clone(),
                &vec![position],
            )
            .unwrap();

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        let info = mock_info(&anyone_str, &[]); // caller is not alice

        // Pass explicit position_owner
        let result = accrue(
            deps.as_mut(),
            env,
            info,
            Some(alice_str),
            vec![Uint128::one()],
        );

        assert!(
            result.is_ok(),
            "Should succeed with explicit owner: {:?}",
            result.err()
        );

        let positions = POSITIONS
            .load(deps.as_ref().storage, alice_addr)
            .unwrap();
        let debt = get_total_position_debt(&positions[0]);
        assert!(debt > Uint128::new(1_000_000));
    }

    #[test]
    fn test_accrue_no_user_positions_errors() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let user_addr = deps.api.addr_make("user");
        let user_str = user_addr.to_string();

        // Don't store any positions for user
        let env = env_at_time(1_000_000 + 86400);
        let info = mock_info(&user_str, &[]);

        let result = accrue(deps.as_mut(), env, info, None, vec![Uint128::one()]);

        assert!(result.is_err(), "Should error when user has no positions");
    }

    // ========================================================================
    // Additional edge case tests
    // ========================================================================

    #[test]
    fn test_accrue_position_two_assets_weighted_rate_of_change() {
        let mut deps = mock_dependencies();
        init_test_state_two_assets(&mut deps);

        let config = default_config();
        let mut basket = default_basket();

        // Position with 50/50 weighting across two assets
        let mut position = DebtPosition {
            position_id: Uint128::one(),
            rate_segments: vec![RateSegment {
                amount: Uint128::new(1_000_000),
                fixed_rate: None,
            }],
            peg_rate_segments: vec![],
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![
                AssetRateIndex {
                    asset_denom: ATOM_DENOM.to_string(),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    ratio: Decimal::percent(50),
                },
                AssetRateIndex {
                    asset_denom: OSMO_DENOM.to_string(),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    ratio: Decimal::percent(50),
                },
            ],
        };

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        let debt_after = get_total_position_debt(&position);
        assert!(debt_after > Uint128::new(1_000_000));

        // Both collateral rate indices should be synced
        assert_eq!(position.collateral_rate_indices.len(), 2);

        let atom_basket_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        let osmo_basket_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, OSMO_DENOM.to_string())
            .unwrap();

        assert_eq!(
            position.collateral_rate_indices[0].rate_index,
            atom_basket_idx.rate_index
        );
        assert_eq!(
            position.collateral_rate_indices[1].rate_index,
            osmo_basket_idx.rate_index
        );
    }

    #[test]
    fn test_accrue_position_large_debt_amount() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        // Large debt: 1 trillion
        let mut position = create_test_position(1_000_000_000_000);

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        let result =
            call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user");

        assert!(
            result.is_ok(),
            "Large debt should not overflow: {:?}",
            result.err()
        );
        let debt_after = get_total_position_debt(&position);
        assert!(debt_after > Uint128::new(1_000_000_000_000));
    }

    #[test]
    fn test_accrue_position_small_time_increment() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        let mut position = create_test_position(1_000_000);

        // Advance only 1 second
        let env = env_at_time(1_000_001);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // With 5% rate over 1 second, the accrual is tiny.
        // Due to integer truncation, debt might not change at all for such small amounts.
        let debt_after = get_total_position_debt(&position);
        // Should at minimum stay the same (not decrease)
        assert!(debt_after >= Uint128::new(1_000_000));
    }

    #[test]
    fn test_update_rate_indices_two_assets() {
        let mut deps = mock_dependencies();
        init_test_state_two_assets(&mut deps);

        let config = default_config();
        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        // Both assets should have grown indices
        let atom_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        let osmo_idx = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, OSMO_DENOM.to_string())
            .unwrap();

        assert!(atom_idx.rate_index > Decimal::one());
        assert!(osmo_idx.rate_index > Decimal::one());

        // Historical rates should exist for both
        let atom_history = crate::state::HISTORICAL_INTEREST_RATES
            .may_load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        let osmo_history = crate::state::HISTORICAL_INTEREST_RATES
            .may_load(deps.as_ref().storage, OSMO_DENOM.to_string())
            .unwrap();
        assert!(atom_history.is_some());
        assert!(osmo_history.is_some());
    }

    #[test]
    fn test_accrue_position_fixed_rate_caps_updated() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let config = default_config();
        let mut basket = default_basket();
        // 3-month fixed rate position
        let mut position = create_fixed_rate_position(
            1_000_000,
            Decimal::percent(10),
            1_000_000 + 10 * SECONDS_PER_YEAR,
            false,
            3,
        );

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);

        call_accrue_position(&mut deps, env, &config, &mut position, &mut basket, "user")
            .unwrap();

        // The fixed_rate_caps in RATES should have been updated with the delta
        let rates = RATES.load(deps.as_ref().storage).unwrap();
        // Three-month cap amount should reflect the accrued delta (100_000 added)
        assert_eq!(
            rates.fixed_rate_caps.three_month.amount,
            Uint128::new(100_000),
            "Fixed rate caps should track 3-month delta"
        );
    }

    #[test]
    fn test_accrue_response_attributes() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let user_addr = deps.api.addr_make("user");
        let user_str = user_addr.to_string();

        let position = create_test_position(1_000_000);
        POSITIONS
            .save(deps.as_mut().storage, user_addr, &vec![position])
            .unwrap();

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        let info = mock_info(&user_str, &[]);

        let response = accrue(deps.as_mut(), env, info, None, vec![Uint128::one()]).unwrap();

        // Check response attributes
        let method_attr = response
            .attributes
            .iter()
            .find(|a| a.key == "method")
            .unwrap();
        assert_eq!(method_attr.value, "accrue");

        let interest_attr = response
            .attributes
            .iter()
            .find(|a| a.key == "accrued_interest")
            .unwrap();
        // Interest should be non-zero
        let accrued: u128 = interest_attr.value.parse().unwrap();
        assert!(accrued > 0, "accrued_interest attribute should be > 0");
    }

    // ========================================================================
    // GROUP E: Acquisition Bump Rate Tests
    //
    // The acquisition bump is added onto current_adaptive_rate as an additive
    // term OUTSIDE the control system. It must:
    //   1. NOT be saved into current_adaptive_rate in the RATES store
    //   2. Appear in lastest_collateral_rates (historical trackers)
    //   3. Appear in historical interest rates
    //   4. Increase rate index accumulation
    //   5. Not poison the control system when bump values change over time
    // ========================================================================

    #[test]
    fn test_acquisition_bump_not_saved_in_current_adaptive_rate() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Set a non-zero acquisition bump
        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2); // 2% bump
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let config = default_config();
        let env = env_at_time(1_000_000 + 86400); // 1 day later

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        // Load RATES from store — current_adaptive_rate should NOT include the bump
        let stored_rates = RATES.load(deps.as_ref().storage).unwrap();

        // The smoothed base rate should be close to the destination (LTV fallback = 2.5%)
        // but should NOT include the bump (2% * 0.8 = 1.6%)
        // If the bump were saved, the stored rate would be ~base + 1.6%
        assert!(
            stored_rates.current_adaptive_rate[0] < Decimal::percent(10),
            "Stored current_adaptive_rate should be the base rate (no bump): {}",
            stored_rates.current_adaptive_rate[0]
        );

        // Verify the bump itself is still stored
        assert_eq!(stored_rates.acquisition_bump_rate, Decimal::percent(2));
    }

    #[test]
    fn test_acquisition_bump_appears_in_lastest_collateral_rates() {
        let config = default_config();

        // Run WITHOUT bump
        let mut deps_no_bump = mock_dependencies();
        init_test_state(&mut deps_no_bump);

        let env = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_no_bump, env, &config).unwrap();

        let rates_no_bump = RATES.load(deps_no_bump.as_ref().storage).unwrap();
        let base_latest_rate = rates_no_bump.lastest_collateral_rates[0].rate;
        let base_stored_rate = rates_no_bump.current_adaptive_rate[0];

        // Run WITH bump (fresh state, same starting conditions)
        let mut deps_with_bump = mock_dependencies();
        init_test_state(&mut deps_with_bump);
        let mut rates = RATES.load(deps_with_bump.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2); // 2% bump
        RATES.save(deps_with_bump.as_mut().storage, &rates).unwrap();

        let env = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_with_bump, env, &config).unwrap();

        let rates_with_bump = RATES.load(deps_with_bump.as_ref().storage).unwrap();
        let bumped_latest_rate = rates_with_bump.lastest_collateral_rates[0].rate;
        let bumped_stored_rate = rates_with_bump.current_adaptive_rate[0];

        // current_adaptive_rate (stored) should be identical — bump not saved there
        assert_eq!(
            base_stored_rate, bumped_stored_rate,
            "Stored current_adaptive_rate should be identical with or without bump"
        );

        // lastest_collateral_rates should NOT include the bump (scoped to peg debt only)
        assert_eq!(
            bumped_latest_rate, base_latest_rate,
            "lastest_collateral_rates should NOT include bump: with_bump={} vs without_bump={}",
            bumped_latest_rate, base_latest_rate
        );

        // Both should equal current_adaptive_rate (base rate)
        assert_eq!(
            base_latest_rate, base_stored_rate,
            "Without bump, latest rate should equal stored base rate"
        );
        assert_eq!(
            bumped_latest_rate, bumped_stored_rate,
            "With bump, latest rate should still equal stored base rate (bump only in peg)"
        );
    }

    #[test]
    fn test_acquisition_bump_does_not_appear_in_historical_rates() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2);
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let config = default_config();
        let target_time = 1_000_000 + 86400;
        let env = env_at_time(target_time);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let stored_rates = RATES.load(deps.as_ref().storage).unwrap();
        let base_rate = stored_rates.current_adaptive_rate[0];

        let history = crate::state::HISTORICAL_INTEREST_RATES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();
        let recorded_rate: Decimal = history[0].rate.parse().unwrap();

        // Historical rates should NOT include the bump (bump scoped to peg debt only)
        assert_eq!(
            recorded_rate, base_rate,
            "Historical rate should be base rate (no bump): {} vs expected {}",
            recorded_rate, base_rate
        );
    }

    #[test]
    fn test_acquisition_bump_scoped_to_peg_rate_index_only() {
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Run without bump
        let config = default_config();
        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let idx_no_bump = BASKET_RATE_INDICES
            .load(deps.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();

        // Reset and run with bump
        let mut deps2 = mock_dependencies();
        init_test_state(&mut deps2);
        let mut rates = RATES.load(deps2.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2);
        RATES.save(deps2.as_mut().storage, &rates).unwrap();

        let env = env_at_time(1_000_000 + SECONDS_PER_YEAR);
        call_update_rate_indices(&mut deps2, env, &config).unwrap();

        let idx_with_bump = BASKET_RATE_INDICES
            .load(deps2.as_ref().storage, ATOM_DENOM.to_string())
            .unwrap();

        // CDT rate_index: IDENTICAL with or without bump — bump is scoped to peg debt only
        assert_eq!(
            idx_with_bump.rate_index, idx_no_bump.rate_index,
            "CDT rate_index must be identical with/without bump: with={} vs without={}",
            idx_with_bump.rate_index,
            idx_no_bump.rate_index
        );

        // Peg rate_index: HIGHER with bump — bump is applied to USDC/peg debt
        assert!(
            idx_with_bump.peg_rate_index > idx_no_bump.peg_rate_index,
            "Peg rate_index must be higher with bump: with={} vs without={}",
            idx_with_bump.peg_rate_index,
            idx_no_bump.peg_rate_index
        );

        // Verify the peg got MORE accrual than CDT (the delta is the bump contribution)
        let cdt_growth = idx_with_bump.rate_index - Decimal::one();
        let peg_growth = idx_with_bump.peg_rate_index - Decimal::one();
        assert!(
            peg_growth > cdt_growth,
            "Peg growth ({}) must exceed CDT growth ({}) due to bump",
            peg_growth, cdt_growth
        );
    }

    #[test]
    fn test_acquisition_bump_no_carryover_across_ticks() {
        // Verify that the bump from tick N does NOT pollute the control system
        // input at tick N+1. Each tick should produce the same stored base rate
        // as a run without any bump at all.
        let config = default_config();

        // --- Baseline: two ticks with NO bump ---
        let mut deps_baseline = mock_dependencies();
        init_test_state(&mut deps_baseline);

        let env1 = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_baseline, env1, &config).unwrap();
        let baseline_after_tick1 = RATES.load(deps_baseline.as_ref().storage).unwrap();

        let env2 = env_at_time(1_000_000 + 2 * 86400);
        call_update_rate_indices(&mut deps_baseline, env2, &config).unwrap();
        let baseline_after_tick2 = RATES.load(deps_baseline.as_ref().storage).unwrap();

        // --- Two ticks WITH bump ---
        let mut deps_bumped = mock_dependencies();
        init_test_state(&mut deps_bumped);
        let mut rates = RATES.load(deps_bumped.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(3);
        RATES.save(deps_bumped.as_mut().storage, &rates).unwrap();

        let env1 = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_bumped, env1, &config).unwrap();

        let bumped_after_tick1 = RATES.load(deps_bumped.as_ref().storage).unwrap();
        assert_eq!(
            bumped_after_tick1.current_adaptive_rate[0],
            baseline_after_tick1.current_adaptive_rate[0],
            "After tick 1, stored base rate should match baseline (bump not carried over)"
        );

        // Tick 2: bump is still active, stored rate from tick 1 should be base
        let env2 = env_at_time(1_000_000 + 2 * 86400);
        call_update_rate_indices(&mut deps_bumped, env2, &config).unwrap();

        let bumped_after_tick2 = RATES.load(deps_bumped.as_ref().storage).unwrap();
        assert_eq!(
            bumped_after_tick2.current_adaptive_rate[0],
            baseline_after_tick2.current_adaptive_rate[0],
            "After tick 2, stored base rate should still match baseline (no carryover)"
        );
    }

    #[test]
    fn test_acquisition_bump_change_over_time() {
        // Start with bump=3%, run a tick, change to bump=1%, run another tick.
        // Verify the stored base rate is always identical to a zero-bump baseline,
        // and lastest_collateral_rates reflects the current bump value.
        let config = default_config();

        // Baseline with zero bump
        let mut deps_baseline = mock_dependencies();
        init_test_state(&mut deps_baseline);

        // Bumped run
        let mut deps_bumped = mock_dependencies();
        init_test_state(&mut deps_bumped);

        // --- Tick 1: bump = 3% ---
        let mut rates = RATES.load(deps_bumped.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(3);
        RATES.save(deps_bumped.as_mut().storage, &rates).unwrap();

        let env1 = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_bumped, env1.clone(), &config).unwrap();
        call_update_rate_indices(&mut deps_baseline, env1, &config).unwrap();

        let stored_1 = RATES.load(deps_bumped.as_ref().storage).unwrap();
        let baseline_1 = RATES.load(deps_baseline.as_ref().storage).unwrap();
        assert_eq!(
            stored_1.current_adaptive_rate[0],
            baseline_1.current_adaptive_rate[0],
            "Tick 1: stored base rate should match zero-bump baseline"
        );

        // --- Tick 2: change bump to 1% ---
        let mut rates = RATES.load(deps_bumped.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(1); // changed!
        RATES.save(deps_bumped.as_mut().storage, &rates).unwrap();

        let env2 = env_at_time(1_000_000 + 2 * 86400);
        call_update_rate_indices(&mut deps_bumped, env2.clone(), &config).unwrap();
        call_update_rate_indices(&mut deps_baseline, env2, &config).unwrap();

        let stored_2 = RATES.load(deps_bumped.as_ref().storage).unwrap();
        let baseline_2 = RATES.load(deps_baseline.as_ref().storage).unwrap();
        assert_eq!(
            stored_2.current_adaptive_rate[0],
            baseline_2.current_adaptive_rate[0],
            "Tick 2: stored base rate should match zero-bump baseline after bump change"
        );

        // lastest_collateral_rates should equal base rate (no bump — bump only in peg)
        assert_eq!(
            stored_2.lastest_collateral_rates[0].rate,
            stored_2.current_adaptive_rate[0],
            "Latest rate should equal base rate (no bump): got {} expected {}",
            stored_2.lastest_collateral_rates[0].rate,
            stored_2.current_adaptive_rate[0]
        );
    }

    #[test]
    fn test_acquisition_bump_removed_no_residual() {
        // Set bump, run a tick, then remove bump (set to zero), run another tick.
        // Verify no residual bump remains anywhere.
        let config = default_config();

        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        // Also run baseline for base rate comparison
        let mut deps_baseline = mock_dependencies();
        init_test_state(&mut deps_baseline);

        // Tick 1: bump = 2%
        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2);
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let env1 = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps, env1.clone(), &config).unwrap();
        call_update_rate_indices(&mut deps_baseline, env1, &config).unwrap();

        // Verify bump is active but doesn't affect lastest_collateral_rates (bump scoped to peg only)
        let rates_after_1 = RATES.load(deps.as_ref().storage).unwrap();
        let base_rate = rates_after_1.current_adaptive_rate[0];
        assert_eq!(
            rates_after_1.lastest_collateral_rates[0].rate,
            base_rate,
            "Tick 1: latest should equal base rate (no bump in lastest_collateral_rates)"
        );
        // Verify the bump itself is still stored
        assert_eq!(rates_after_1.acquisition_bump_rate, Decimal::percent(2));

        // Verify stored base rate matches baseline
        let baseline_1 = RATES.load(deps_baseline.as_ref().storage).unwrap();
        assert_eq!(
            rates_after_1.current_adaptive_rate[0],
            baseline_1.current_adaptive_rate[0],
            "Tick 1: stored base should match zero-bump baseline"
        );

        // Tick 2: remove bump
        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::zero();
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let env2 = env_at_time(1_000_000 + 2 * 86400);
        call_update_rate_indices(&mut deps, env2.clone(), &config).unwrap();
        call_update_rate_indices(&mut deps_baseline, env2, &config).unwrap();

        let rates_after_2 = RATES.load(deps.as_ref().storage).unwrap();
        let baseline_2 = RATES.load(deps_baseline.as_ref().storage).unwrap();

        // With bump removed, lastest_collateral_rates should equal current_adaptive_rate
        assert_eq!(
            rates_after_2.lastest_collateral_rates[0].rate,
            rates_after_2.current_adaptive_rate[0],
            "After bump removed, latest rate should equal base rate (no residual)"
        );

        // Stored base rate should still match the zero-bump baseline
        assert_eq!(
            rates_after_2.current_adaptive_rate[0],
            baseline_2.current_adaptive_rate[0],
            "After bump removed, stored rate should match baseline (no residual)"
        );
    }

    #[test]
    fn test_acquisition_bump_two_assets_different_ltv() {
        // Two assets with different max_ltv should get different bump amounts.
        // Run a baseline alongside to verify stored base rates match.
        let config = default_config();

        let mut deps_bumped = mock_dependencies();
        init_test_state_two_assets(&mut deps_bumped);
        let mut rates = RATES.load(deps_bumped.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(5); // 5% bump
        RATES.save(deps_bumped.as_mut().storage, &rates).unwrap();

        let mut deps_baseline = mock_dependencies();
        init_test_state_two_assets(&mut deps_baseline);

        let env = env_at_time(1_000_000 + 86400);
        call_update_rate_indices(&mut deps_bumped, env.clone(), &config).unwrap();
        call_update_rate_indices(&mut deps_baseline, env, &config).unwrap();

        let stored = RATES.load(deps_bumped.as_ref().storage).unwrap();
        let baseline = RATES.load(deps_baseline.as_ref().storage).unwrap();

        // Stored base rates should match baseline (no bump in store)
        assert_eq!(
            stored.current_adaptive_rate[0],
            baseline.current_adaptive_rate[0],
            "ATOM stored base should match baseline"
        );
        assert_eq!(
            stored.current_adaptive_rate[1],
            baseline.current_adaptive_rate[1],
            "OSMO stored base should match baseline"
        );

        // lastest_collateral_rates should equal base rates (no bump — bump only in peg)
        let atom_base = stored.current_adaptive_rate[0];
        assert_eq!(
            stored.lastest_collateral_rates[0].rate, atom_base,
            "ATOM latest should equal base rate (no bump in lastest_collateral_rates)"
        );

        let osmo_base = stored.current_adaptive_rate[1];
        assert_eq!(
            stored.lastest_collateral_rates[1].rate, osmo_base,
            "OSMO latest should equal base rate (no bump in lastest_collateral_rates)"
        );
    }

    #[test]
    fn test_acquisition_bump_peg_rate_mirrors_bumped_rate() {
        // peg_current_adaptive_rate in-memory should also get the bump post-save
        let mut deps = mock_dependencies();
        init_test_state(&mut deps);

        let mut rates = RATES.load(deps.as_ref().storage).unwrap();
        rates.acquisition_bump_rate = Decimal::percent(2);
        RATES.save(deps.as_mut().storage, &rates).unwrap();

        let config = default_config();
        let env = env_at_time(1_000_000 + 86400);

        call_update_rate_indices(&mut deps, env, &config).unwrap();

        let stored = RATES.load(deps.as_ref().storage).unwrap();

        // In the store, peg should mirror the base (no bump)
        assert_eq!(
            stored.peg_current_adaptive_rate[0],
            stored.current_adaptive_rate[0],
            "Stored peg rate should equal stored base rate (both without bump)"
        );
    }
}
