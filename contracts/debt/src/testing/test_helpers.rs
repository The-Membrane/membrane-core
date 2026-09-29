/// Shared test infrastructure for the Debt contract.
///
/// Provides helper functions to initialize state items used across
/// integration, financial, and property-based tests.
///
/// NOTE: No `#[cfg(test)]` needed here since the parent `testing` module
/// is already gated behind `#[cfg(test)]` in lib.rs.
pub mod helpers {
    use cosmwasm_std::{
        Addr, Decimal, Event, Reply, Storage, SubMsgResponse, SubMsgResult, Uint128,
    };

    use membrane::debt::{BasketDebt, Config, DebtPosition};
    use membrane::oracle::PriceResponse;
    use membrane::types::{
        Asset, AssetInfo, CreditAssetBreakdown, FixedRateCap, FixedRateCaps, IRMConfig,
        PendingRevenue, RateSegment, Rates,
    };

    use crate::rates::CollateralTypeInfo;
    use crate::rates::COLLATERAL_TYPES_CACHE;
    use crate::state::{
        BasketRateIndex, BASKET, BASKET_RATE_INDICES, CDT_SUPPLY, CONFIG, FROZEN, POSITIONS, RATES,
    };

    // ========================================================================
    // Rates
    // ========================================================================

    /// Initialize RATES with sensible defaults for testing.
    ///
    /// - base_interest_rate: 2%
    /// - fixed_rate_caps: 20% cap (1mo/3mo), 10% (6mo); multipliers 1.1/1.3/1.7
    /// - rates_last_accrued / credit_last_accrued: 1000 (low timestamp)
    /// - current_adaptive_rate: [3%]
    /// - peg_current_adaptive_rate: [3%]
    /// - lastest_collateral_rates: empty
    /// - cpc_margin_of_error: 1.0
    /// - negative_rates: false
    pub fn init_rates(storage: &mut dyn Storage) -> Rates {
        let rates = Rates {
            base_interest_rate: Decimal::percent(2),
            fixed_rate_caps: FixedRateCaps {
                one_month: FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::from_ratio(11u128, 10u128), // 1.1
                },
                three_month: FixedRateCap {
                    cap: Decimal::percent(20),
                    amount: Uint128::zero(),
                    multiplier: Decimal::from_ratio(13u128, 10u128), // 1.3
                },
                six_month: FixedRateCap {
                    cap: Decimal::percent(10),
                    amount: Uint128::zero(),
                    multiplier: Decimal::from_ratio(17u128, 10u128), // 1.7
                },
            },
            rates_last_accrued: 1000u64,
            credit_last_accrued: 1000u64,
            current_adaptive_rate: vec![Decimal::percent(3)],
            peg_current_adaptive_rate: vec![Decimal::percent(3)],
            lastest_collateral_rates: vec![],
            cpc_margin_of_error: Decimal::one(),
            negative_rates: false,
            acquisition_bump_rate: Decimal::zero(),
        };
        RATES.save(storage, &rates).unwrap();
        rates
    }

    // ========================================================================
    // Config
    // ========================================================================

    /// Initialize CONFIG with skip_credit_price_accrual: true and contracts
    /// using `Addr::unchecked()` for test addresses.
    pub fn init_config(storage: &mut dyn Storage) -> Config {
        let config = Config {
            owner: Addr::unchecked("admin"),
            collateral_contract: Some(Addr::unchecked("collateral_contract")),
            liquidation_contract: Some(Addr::unchecked("liquidation_contract")),
            staking_contract: None,
            chain_proxy: None,
            debt_auction: None,
            oracle_contract: None,
            liquidity_contract: None,
            discounts_contract: None,
            revenue_distributor: None,
            transmuter_addr: None,
            points_contract: Some(Addr::unchecked("points_contract")),
            liq_fee: Decimal::percent(1),
            credit_twap_timeframe: 60u64,
            cpc_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(100_000u128),
            base_debt_cap_multiplier: Uint128::new(10u128),
            rate_slope_multiplier: Decimal::one(),
            affiliate_fee_max: Decimal::percent(5),
            skip_credit_price_accrual: true,
            liquidation_stat_limit: 100u64,
            irm_config: IRMConfig {
                adjustment_speed: Decimal::from_ratio(50u128, 1u128),
                min_adaptive_rate: Decimal::permille(1), // 0.1%
                max_adaptive_rate: Decimal::from_ratio(2u128, 1u128), // 200%
            },
            acquisition_contract: None,
        };
        CONFIG.save(storage, &config).unwrap();
        config
    }

    // ========================================================================
    // Basket
    // ========================================================================

    /// Initialize BASKET with credit_asset CDT, credit_price = 1.0, zero amounts.
    pub fn init_basket(storage: &mut dyn Storage) -> BasketDebt {
        let basket = BasketDebt {
            basket_id: Uint128::one(),
            credit_asset: CreditAssetBreakdown {
                info: AssetInfo::NativeToken {
                    denom: "factory/creator/ucdt".to_string(),
                },
                variable_amount: Uint128::zero(),
                one_month_amount: Uint128::zero(),
                three_month_amount: Uint128::zero(),
                six_month_amount: Uint128::zero(),
                peg_variable_amount: Uint128::zero(),
                peg_one_month_amount: Uint128::zero(),
                peg_three_month_amount: Uint128::zero(),
                peg_six_month_amount: Uint128::zero(),
            },
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6u64,
            },
            pending_revenue: PendingRevenue {
                total_pending: Uint128::zero(),
                per_asset_rev: vec![],
            },
            pending_bad_debt: Uint128::zero(),
            liq_queue: None,
            oracle_set: false,
            distribute_revenue: false,
        };
        BASKET.save(storage, &basket).unwrap();
        basket
    }

    // ========================================================================
    // Frozen State
    // ========================================================================

    /// Initialize FROZEN state.
    pub fn init_frozen_state(storage: &mut dyn Storage, frozen: bool) {
        FROZEN.save(storage, &frozen).unwrap();
    }

    // ========================================================================
    // CDT Supply
    // ========================================================================

    /// Initialize CDT_SUPPLY.
    pub fn init_cdt_supply(storage: &mut dyn Storage, amount: Uint128) {
        CDT_SUPPLY.save(storage, &amount).unwrap();
    }

    // ========================================================================
    // Basket Rate Index
    // ========================================================================

    /// Initialize a BASKET_RATE_INDICES entry for a given asset denom.
    /// rate_index = 1.0, peg_rate_index = 1.0
    pub fn init_basket_rate_index(storage: &mut dyn Storage, asset_denom: &str) {
        let index = BasketRateIndex {
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
        };
        BASKET_RATE_INDICES
            .save(storage, asset_denom.to_string(), &index)
            .unwrap();
    }

    // ========================================================================
    // Collateral Types Cache
    // ========================================================================

    /// Initialize COLLATERAL_TYPES_CACHE with a single asset entry.
    pub fn init_collateral_types_cache(storage: &mut dyn Storage, asset_denom: &str, max_ltv: Decimal) {
        let cache = vec![CollateralTypeInfo {
            asset_denom: asset_denom.to_string(),
            max_ltv,
        }];
        COLLATERAL_TYPES_CACHE.save(storage, &cache).unwrap();
    }

    // ========================================================================
    // Positions
    // ========================================================================

    /// Create a DebtPosition with variable-rate segments and save it to POSITIONS.
    ///
    /// - `var_amount`: amount in the variable rate segment (rate_segments)
    /// - `peg_var_amount`: amount in the peg variable rate segment (peg_rate_segments)
    pub fn create_position_with_debt(
        storage: &mut dyn Storage,
        owner: Addr,
        position_id: Uint128,
        var_amount: u128,
        peg_var_amount: u128,
    ) -> DebtPosition {
        let mut rate_segments = vec![];
        if var_amount > 0 {
            rate_segments.push(RateSegment {
                amount: Uint128::new(var_amount),
                fixed_rate: None,
            });
        }

        let mut peg_rate_segments = vec![];
        if peg_var_amount > 0 {
            peg_rate_segments.push(RateSegment {
                amount: Uint128::new(peg_var_amount),
                fixed_rate: None,
            });
        }

        let position = DebtPosition {
            position_id,
            rate_segments,
            peg_rate_segments,
            deployed_to: vec![],
            pending_interest: Uint128::zero(),
            total_interest_accrued: Uint128::zero(),
            vol_window_initial_debt: None,
            collateral_rate_indices: vec![],
        };

        // Load existing positions for the owner or start fresh
        let mut positions = POSITIONS
            .may_load(storage, owner.clone())
            .unwrap()
            .unwrap_or_default();
        positions.push(position.clone());
        POSITIONS.save(storage, owner, &positions).unwrap();

        position
    }

    // ========================================================================
    // Reply Helpers
    // ========================================================================

    /// Create a successful Reply with the given id and event attributes.
    pub fn make_success_reply(id: u64, attrs: Vec<(&str, &str)>) -> Reply {
        let event = Event::new("wasm").add_attributes(
            attrs
                .into_iter()
                .map(|(k, v)| cosmwasm_std::Attribute::new(k, v))
                .collect::<Vec<_>>(),
        );
        Reply {
            id,
            payload: Default::default(),
            gas_used: 0,
            result: SubMsgResult::Ok(SubMsgResponse {
                events: vec![event],
                msg_responses: vec![],
                data: None,
            }),
        }
    }

    /// Create a failed Reply with the given id and error message.
    pub fn make_error_reply(id: u64, error: &str) -> Reply {
        Reply {
            id,
            payload: Default::default(),
            gas_used: 0,
            result: SubMsgResult::Err(error.to_string()),
        }
    }
}
