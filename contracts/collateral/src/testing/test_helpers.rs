/// Shared test infrastructure for the Collateral contract.
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

    use membrane::collateral::{BasketCollateral, CollateralPosition, Config};
    use membrane::oracle::PriceResponse;
    use membrane::types::{Asset, AssetInfo, SupplyCap, MultiAssetSupplyCap, UserInfo, cAsset};

    use crate::state::{
        AssetCircuitBreaker, CollateralVolatility,
        PriceTimestamp, SellCollateralPropagation, StoredPrice, WithdrawPropagation,
        ASSET_CIRCUIT_BREAKERS, BASKET, CONFIG, FROZEN, HISTORICAL_ORACLE_PRICES,
        POSITIONS, STORED_PRICES, VOLATILITY,
        SELL_COLLATERAL_PROPAGATION, WITHDRAW_PROPAGATION,
    };

    // ========================================================================
    // Config
    // ========================================================================

    /// Initialize CONFIG with test defaults.
    ///
    /// - owner: "admin"
    /// - oracle_contract: Some("oracle_contract")
    /// - chain_proxy: Some("chain_proxy")
    /// - ltv_disco: "ltv_disco"
    /// - debt_contract: Some("debt_contract")
    /// - liquidation_contract: Some("liquidation_contract")
    /// - collateral_twap_timeframe: 60
    /// - oracle_time_limit: 600
    pub fn init_config(storage: &mut dyn Storage) -> Config {
        let config = Config {
            owner: Addr::unchecked("admin"),
            oracle_contract: Some(Addr::unchecked("oracle_contract")),
            chain_proxy: Some(Addr::unchecked("chain_proxy")),
            ltv_disco: Addr::unchecked("ltv_disco"),
            debt_contract: Some(Addr::unchecked("debt_contract")),
            liquidation_contract: Some(Addr::unchecked("liquidation_contract")),
            collateral_twap_timeframe: 60u64,
            oracle_time_limit: 600u64,
            ltv_upward_kp: Decimal::percent(5),
            ltv_downward_period: 604_800u64,
            ltv_downward_decay_rate: Decimal::percent(40),
            ltv_borrow_distance: Decimal::percent(10),
        };
        CONFIG.save(storage, &config).unwrap();
        config
    }

    // ========================================================================
    // Basket
    // ========================================================================

    /// Initialize BASKET with 2 collateral types (uatom 50%/60%, uosmo 40%/50%),
    /// supply caps at 100%, and empty multi_asset_supply_caps.
    pub fn init_basket(storage: &mut dyn Storage) -> BasketCollateral {
        let basket = BasketCollateral {
            basket_id: Uint128::one(),
            current_position_id: Uint128::zero(),
            collateral_types: vec![
                make_casset("uatom", 50, 60),
                make_casset("uosmo", 40, 50),
            ],
            collateral_supply_caps: vec![
                make_supply_cap("uatom"),
                make_supply_cap("uosmo"),
            ],
            multi_asset_supply_caps: vec![],
        };
        BASKET.save(storage, &basket).unwrap();
        basket
    }

    /// Initialize BASKET with custom collateral types and supply caps.
    pub fn init_basket_custom(
        storage: &mut dyn Storage,
        collateral_types: Vec<cAsset>,
        supply_caps: Vec<SupplyCap>,
        multi_asset_supply_caps: Vec<MultiAssetSupplyCap>,
    ) -> BasketCollateral {
        let basket = BasketCollateral {
            basket_id: Uint128::one(),
            current_position_id: Uint128::zero(),
            collateral_types,
            collateral_supply_caps: supply_caps,
            multi_asset_supply_caps,
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
    // Positions
    // ========================================================================

    /// Create a CollateralPosition and save it to POSITIONS.
    pub fn init_position(
        storage: &mut dyn Storage,
        owner: Addr,
        position_id: Uint128,
        collateral_assets: Vec<cAsset>,
    ) -> CollateralPosition {
        let position = CollateralPosition {
            position_id,
            collateral_assets,
        };

        let mut positions = POSITIONS
            .may_load(storage, owner.clone())
            .unwrap()
            .unwrap_or_default();
        positions.push(position.clone());
        POSITIONS.save(storage, owner, &positions).unwrap();

        position
    }

    // ========================================================================
    // cAsset Builders
    // ========================================================================

    /// Build a cAsset with zero amount and specified LTV percentages.
    pub fn make_casset(denom: &str, borrow_ltv_pct: u64, max_ltv_pct: u64) -> cAsset {
        cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: denom.to_string(),
                },
                amount: Uint128::zero(),
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

    /// Build a cAsset with a specific amount and LTV percentages.
    pub fn make_casset_with_amount(
        denom: &str,
        amount: u128,
        borrow_ltv_pct: u64,
        max_ltv_pct: u64,
    ) -> cAsset {
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

    /// Build a cAsset with a specific amount, LTV percentages, and custom threshold.
    pub fn make_casset_with_threshold(
        denom: &str,
        amount: u128,
        borrow_ltv_pct: u64,
        max_ltv_pct: u64,
        threshold_pct: u64,
    ) -> cAsset {
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

    // ========================================================================
    // Supply Cap Builders
    // ========================================================================

    /// Build a SupplyCap with 100% ratio, zero current supply.
    pub fn make_supply_cap(denom: &str) -> SupplyCap {
        SupplyCap {
            asset_info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            current_supply: Uint128::zero(),
            debt_total: Uint128::zero(),
            supply_cap_ratio: Decimal::one(), // 100%
            lp: false,
            stability_pool_ratio_for_debt_cap: None,
        }
    }

    /// Build a SupplyCap with a custom ratio percentage.
    pub fn make_supply_cap_with_ratio(denom: &str, ratio_pct: u64) -> SupplyCap {
        SupplyCap {
            asset_info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            current_supply: Uint128::zero(),
            debt_total: Uint128::zero(),
            supply_cap_ratio: Decimal::percent(ratio_pct),
            lp: false,
            stability_pool_ratio_for_debt_cap: None,
        }
    }

    /// Build a SupplyCap with a custom ratio and current supply.
    pub fn make_supply_cap_with_supply(
        denom: &str,
        ratio_pct: u64,
        current_supply: u128,
    ) -> SupplyCap {
        SupplyCap {
            asset_info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            current_supply: Uint128::new(current_supply),
            debt_total: Uint128::zero(),
            supply_cap_ratio: Decimal::percent(ratio_pct),
            lp: false,
            stability_pool_ratio_for_debt_cap: None,
        }
    }

    // ========================================================================
    // Stored Prices
    // ========================================================================

    /// Seed STORED_PRICES with a price for a given denom.
    pub fn init_stored_price(
        storage: &mut dyn Storage,
        denom: &str,
        price: Decimal,
        time: u64,
    ) {
        let stored_price = StoredPrice {
            price: PriceResponse {
                prices: vec![],
                price,
                decimals: 6u64,
            },
            last_time_updated: time,
        };
        STORED_PRICES
            .save(storage, denom.to_string(), &stored_price)
            .unwrap();
    }

    // ========================================================================
    // Volatility
    // ========================================================================

    /// Seed VOLATILITY for a given denom.
    pub fn init_volatility(
        storage: &mut dyn Storage,
        denom: &str,
        index: Decimal,
        volatility_list: Vec<Decimal>,
        raw_volatility_list: Vec<Decimal>,
    ) {
        let vol = CollateralVolatility {
            index,
            volatility_list,
            raw_volatility_list,
        };
        VOLATILITY
            .save(storage, denom.to_string(), &vol)
            .unwrap();
    }

    // ========================================================================
    // Circuit Breaker
    // ========================================================================

    /// Seed ASSET_CIRCUIT_BREAKERS for a given denom.
    pub fn init_circuit_breaker(
        storage: &mut dyn Storage,
        denom: &str,
        frozen: bool,
        frozen_at: u64,
        threshold: Decimal,
        reference_price: Option<Decimal>,
    ) {
        let breaker = AssetCircuitBreaker {
            frozen,
            frozen_at,
            price_deviation_threshold: threshold,
            reference_price,
        };
        ASSET_CIRCUIT_BREAKERS
            .save(storage, denom.to_string(), &breaker)
            .unwrap();
    }

    // ========================================================================
    // Historical Prices
    // ========================================================================

    /// Seed HISTORICAL_ORACLE_PRICES for a given denom.
    pub fn init_historical_prices(
        storage: &mut dyn Storage,
        denom: &str,
        prices: Vec<(String, u64)>,
    ) {
        let history: Vec<PriceTimestamp> = prices
            .into_iter()
            .map(|(price, timestamp)| PriceTimestamp { price, timestamp })
            .collect();
        HISTORICAL_ORACLE_PRICES
            .save(storage, denom.to_string(), &history)
            .unwrap();
    }

    // ========================================================================
    // Propagation State
    // ========================================================================

    /// Seed WITHDRAW_PROPAGATION for reply tests.
    pub fn save_withdraw_propagation(
        storage: &mut dyn Storage,
        position_owner: &str,
        position_id: Uint128,
        prev_collateral: Vec<Asset>,
        withdraw_amounts: Vec<Uint128>,
        contracts_prev: Vec<Uint128>,
    ) {
        let propagation = WithdrawPropagation {
            positions_prev_collateral: prev_collateral,
            withdraw_amounts,
            contracts_prev_collateral_amount: contracts_prev,
            position_info: UserInfo {
                position_id,
                position_owner: position_owner.to_string(),
            },
        };
        WITHDRAW_PROPAGATION.save(storage, &propagation).unwrap();
    }

    /// Seed SELL_COLLATERAL_PROPAGATION for reply tests.
    pub fn save_sell_collateral_propagation(
        storage: &mut dyn Storage,
        position_owner: &str,
        position_id: Uint128,
        collateral_sold: Vec<cosmwasm_std::Coin>,
    ) {
        let propagation = SellCollateralPropagation {
            collateral_sold,
            position_info: UserInfo {
                position_id,
                position_owner: position_owner.to_string(),
            },
        };
        SELL_COLLATERAL_PROPAGATION
            .save(storage, &propagation)
            .unwrap();
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
