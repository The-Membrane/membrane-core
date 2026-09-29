#![allow(non_snake_case)]

pub mod tests {

    use membrane::debt::*;
    use membrane::oracle::PriceResponse;
    use membrane::types::{
        Asset, AssetInfo, DeploymentIntent, IRMConfig, Rates, UserDeploymentIntents, UserInfo,
    };

    use cosmwasm_std::{
        coin, Addr, Decimal, Empty, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

    const ADMIN: &str = "admin";
    const USER: &str = "user";
    const USER2: &str = "user2";
    const COLLATERAL_CONTRACT: &str = "collateral_contract";
    const LIQUIDATION_CONTRACT: &str = "liquidation_contract";
    const POINTS_CONTRACT: &str = "points_contract";
    const CHAIN_PROXY: &str = "chain_proxy";
    const ORACLE_CONTRACT: &str = "oracle_contract";
    const STAKING_CONTRACT: &str = "staking_contract";
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
            router.bank.init_balance(storage, &api.addr_make(ADMIN), bigger_bank.clone()).unwrap();
            router.bank.init_balance(storage, &api.addr_make(USER), bigger_bank.clone()).unwrap();
            router.bank.init_balance(storage, &api.addr_make(USER2), bigger_bank).unwrap();
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
            staking_contract: Some(get_addr(app, STAKING_CONTRACT).to_string()),
            chain_proxy: Some(get_addr(app, CHAIN_PROXY).to_string()),
            debt_auction: None,
            oracle_contract: Some(get_addr(app, ORACLE_CONTRACT).to_string()),
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

    fn query_config(app: &App, debt_addr: &Addr) -> Config {
        app.wrap().query_wasm_smart(debt_addr.clone(), &QueryMsg::Config {}).unwrap()
    }

    fn query_basket(app: &App, debt_addr: &Addr) -> BasketDebt {
        app.wrap().query_wasm_smart(debt_addr.clone(), &QueryMsg::GetBasket {}).unwrap()
    }

    fn query_frozen(app: &App, debt_addr: &Addr) -> bool {
        app.wrap().query_wasm_smart(debt_addr.clone(), &QueryMsg::GetFrozen {}).unwrap()
    }

    fn query_debt_positions(app: &App, debt_addr: &Addr, user: &str) -> DebtPositionsResponse {
        let user_addr = get_addr(app, user);
        app.wrap()
            .query_wasm_smart(
                debt_addr.clone(),
                &QueryMsg::GetDebtPositions {
                    start_after: None, limit: None, user_info: None, user: Some(user_addr.to_string()),
                },
            )
            .unwrap()
    }

    fn query_debt_position_by_id(app: &App, debt_addr: &Addr, owner: &str, position_id: Uint128) -> DebtPositionsResponse {
        let owner_addr = get_addr(app, owner);
        app.wrap()
            .query_wasm_smart(
                debt_addr.clone(),
                &QueryMsg::GetDebtPositions {
                    start_after: None, limit: None,
                    user_info: Some(UserInfo { position_id, position_owner: owner_addr.to_string() }),
                    user: None,
                },
            )
            .unwrap()
    }

    fn query_credit_rate(app: &App, debt_addr: &Addr) -> PriceResponse {
        app.wrap().query_wasm_smart(debt_addr.clone(), &QueryMsg::GetCreditRate {}).unwrap()
    }

    fn query_rates(app: &App, debt_addr: &Addr) -> Option<Rates> {
        app.wrap().query_wasm_smart(debt_addr.clone(), &QueryMsg::GetRates {}).unwrap()
    }

    // ========================================================================
    // Instantiation Tests
    // ========================================================================

    #[test]
    fn test_proper_instantiation() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, liquidation) = instantiate_debt_contract(&mut app);
        let config = query_config(&app, &debt_addr);
        assert_eq!(config.owner, admin);
        assert_eq!(config.collateral_contract, Some(collateral));
        assert_eq!(config.liquidation_contract, Some(liquidation));
        assert_eq!(config.debt_minimum, Uint128::new(2_000_000_000));
        assert_eq!(config.liq_fee, Decimal::percent(10));
        assert_eq!(config.affiliate_fee_max, Decimal::percent(5));
        assert_eq!(config.skip_credit_price_accrual, false);
        assert_eq!(config.liquidation_stat_limit, 1000);
        assert_eq!(config.points_contract, Some(get_addr(&app, POINTS_CONTRACT)));

        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.basket_id, Uint128::one());
        assert_eq!(basket.credit_price.price, Decimal::one());
        assert_eq!(basket.pending_bad_debt, Uint128::zero());
        assert_eq!(basket.oracle_set, false);
        assert_eq!(basket.distribute_revenue, false);

        assert_eq!(query_frozen(&app, &debt_addr), false);
        let rates = query_rates(&app, &debt_addr);
        assert!(rates.is_none());
    }

    #[test]
    fn test_instantiation_default_owner() {
        let mut app = mock_app();
        let debt_code_id = app.store_code(debt_contract());
        let admin = get_addr(&app, ADMIN);
        let mut msg = default_instantiate_msg(&app);
        msg.owner = None;
        let debt_addr = app.instantiate_contract(debt_code_id, admin.clone(), &msg, &[], "debt", None).unwrap();
        let config = query_config(&app, &debt_addr);
        assert_eq!(config.owner, admin, "Owner should default to sender");
    }

    // ========================================================================
    // SetFrozen Tests
    // ========================================================================

    #[test]
    fn test_set_frozen_by_owner() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), true);
        app.execute_contract(admin, debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: false }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), false);
    }

    #[test]
    fn test_set_frozen_by_collateral_contract() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        app.execute_contract(collateral.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), true);
    }

    #[test]
    fn test_set_frozen_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // UpdateConfig Tests
    // ========================================================================

    #[test]
    fn test_update_config_valid() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let new_owner = get_addr(&app, "new_owner");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: Some(new_owner.to_string()), collateral_contract: None, liquidation_contract: None,
            staking_contract: None, chain_proxy: None, debt_auction: None, oracle_contract: None,
            liquidity_contract: None, discounts_contract: None, revenue_distributor: None,
            transmuter_addr: None, points_contract: None,
            liq_fee: Some(Decimal::percent(15)), credit_twap_timeframe: None, cpc_multiplier: None,
            debt_minimum: Some(Uint128::new(5_000_000_000)), base_debt_cap_multiplier: None,
            rate_slope_multiplier: None, affiliate_fee_max: None, skip_credit_price_accrual: None,
            liquidation_stat_limit: None, irm_config: None, acquisition_contract: None,
        }), &[]).unwrap();
        let config = query_config(&app, &debt_addr);
        assert_eq!(config.owner, new_owner);
        assert_eq!(config.liq_fee, Decimal::percent(15));
        assert_eq!(config.debt_minimum, Uint128::new(5_000_000_000));
    }

    #[test]
    fn test_update_config_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None, collateral_contract: None, liquidation_contract: None,
            staking_contract: None, chain_proxy: None, debt_auction: None, oracle_contract: None,
            liquidity_contract: None, discounts_contract: None, revenue_distributor: None,
            transmuter_addr: None, points_contract: None,
            liq_fee: Some(Decimal::percent(99)), credit_twap_timeframe: None, cpc_multiplier: None,
            debt_minimum: None, base_debt_cap_multiplier: None, rate_slope_multiplier: None,
            affiliate_fee_max: None, skip_credit_price_accrual: None, liquidation_stat_limit: None,
            irm_config: None, acquisition_contract: None,
        }), &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_update_config_all_fields() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let new_collateral = get_addr(&app, "new_collateral");
        let new_liq = get_addr(&app, "new_liquidation");
        let new_points = get_addr(&app, "new_points");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None,
            collateral_contract: Some(new_collateral.to_string()),
            liquidation_contract: Some(new_liq.to_string()),
            staking_contract: None, chain_proxy: None, debt_auction: None, oracle_contract: None,
            liquidity_contract: None, discounts_contract: None, revenue_distributor: None,
            transmuter_addr: None,
            points_contract: Some(new_points.to_string()),
            liq_fee: None, credit_twap_timeframe: Some(120), cpc_multiplier: Some(Decimal::percent(200)),
            debt_minimum: None, base_debt_cap_multiplier: Some(Uint128::new(20)),
            rate_slope_multiplier: Some(Decimal::percent(150)),
            affiliate_fee_max: Some(Decimal::percent(10)),
            skip_credit_price_accrual: Some(true), liquidation_stat_limit: Some(500),
            irm_config: Some(IRMConfig {
                adjustment_speed: Decimal::from_ratio(100u128, 1u128),
                min_adaptive_rate: Decimal::permille(5),
                max_adaptive_rate: Decimal::from_ratio(3u128, 1u128),
            }),
            acquisition_contract: None,
        }), &[]).unwrap();
        let config = query_config(&app, &debt_addr);
        assert_eq!(config.collateral_contract, Some(new_collateral));
        assert_eq!(config.liquidation_contract, Some(new_liq));
        assert_eq!(config.points_contract, Some(new_points));
        assert_eq!(config.credit_twap_timeframe, 120);
        assert_eq!(config.skip_credit_price_accrual, true);
        assert_eq!(config.liquidation_stat_limit, 500);
    }

    // ========================================================================
    // EditBasket Tests
    // ========================================================================

    #[test]
    fn test_edit_basket_valid() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let liq_queue = get_addr(&app, "liq_queue");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::EditBasket(EditBasketDebt {
            liq_queue: Some(liq_queue.to_string()), credit_pool_infos: None,
            credit_asset_twap_price_source: None, base_interest_rate: None, negative_rates: None,
            cpc_margin_of_error: None, distribute_revenue: Some(true), take_revenue: None,
        }), &[]).unwrap();
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.liq_queue, Some(liq_queue));
        assert_eq!(basket.distribute_revenue, true);
    }

    #[test]
    fn test_edit_basket_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::EditBasket(EditBasketDebt {
            liq_queue: None, credit_pool_infos: None, credit_asset_twap_price_source: None,
            base_interest_rate: None, negative_rates: None, cpc_margin_of_error: None,
            distribute_revenue: Some(true), take_revenue: None,
        }), &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_edit_basket_partial_update() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let liq_queue = get_addr(&app, "liq_queue");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::EditBasket(EditBasketDebt {
            liq_queue: Some(liq_queue.to_string()), credit_pool_infos: None,
            credit_asset_twap_price_source: None, base_interest_rate: None, negative_rates: None,
            cpc_margin_of_error: None, distribute_revenue: None, take_revenue: None,
        }), &[]).unwrap();
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::EditBasket(EditBasketDebt {
            liq_queue: None, credit_pool_infos: None, credit_asset_twap_price_source: None,
            base_interest_rate: None, negative_rates: None, cpc_margin_of_error: None,
            distribute_revenue: Some(true), take_revenue: None,
        }), &[]).unwrap();
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.liq_queue, Some(liq_queue));
        assert_eq!(basket.distribute_revenue, true);
    }

    // ========================================================================
    // NotifyDeposit Tests
    // ========================================================================

    #[test]
    fn test_notify_deposit_creates_position() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        let resp = query_debt_positions(&app, &debt_addr, USER);
        assert_eq!(resp.positions.len(), 1);
        assert_eq!(resp.positions[0].position_id, Uint128::one());
        assert_eq!(resp.positions[0].credit_amount, Uint128::zero());
        assert!(resp.positions[0].rate_segments.is_empty());
        assert!(resp.positions[0].peg_rate_segments.is_empty());
    }

    #[test]
    fn test_notify_deposit_idempotent() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        let resp = query_debt_positions(&app, &debt_addr, USER);
        assert_eq!(resp.positions.len(), 1, "Should not duplicate position on second notify");
    }

    #[test]
    fn test_notify_deposit_multiple_positions() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::new(2), USER);
        let resp = query_debt_positions(&app, &debt_addr, USER);
        assert_eq!(resp.positions.len(), 2);
        assert_eq!(resp.positions[0].position_id, Uint128::one());
        assert_eq!(resp.positions[1].position_id, Uint128::new(2));
    }

    #[test]
    fn test_notify_deposit_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let owner = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::NotifyDeposit {
            position_id: Uint128::one(), position_owner: owner.to_string(), affiliate_address: None,
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_notify_deposit_with_affiliate() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let owner = get_addr(&app, USER);
        let affiliate = get_addr(&app, "affiliate");
        app.execute_contract(collateral.clone(), debt_addr.clone(), &ExecuteMsg::NotifyDeposit {
            position_id: Uint128::one(), position_owner: owner.to_string(),
            affiliate_address: Some(affiliate.to_string()),
        }, &[]).unwrap();
        let affiliate_data: Option<membrane::types::AffiliateData> = app.wrap()
            .query_wasm_smart(debt_addr.clone(), &QueryMsg::GetAffiliates { position_id: Uint128::one() })
            .unwrap();
        assert!(affiliate_data.is_some());
        assert_eq!(affiliate_data.unwrap().affiliate_address, affiliate.to_string());
    }

    // ========================================================================
    // Authorization Tests
    // ========================================================================

    #[test]
    fn test_accrue_and_validate_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::AccrueAndValidate {
            position_owner: get_addr(&app, USER).to_string(), position_id: Uint128::one(),
            remaining_collateral_value: Decimal::one(), avg_max_borrow_ltv: Decimal::percent(50),
            avg_max_ltv: Decimal::percent(60),
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_accrue_and_return_debt_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::AccrueAndReturnDebt {
            position_owner: get_addr(&app, USER).to_string(), position_id: Uint128::one(),
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_liquidation_debt_update_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::LiquidationDebtUpdate {
            position_id: Uint128::one(), position_owner: get_addr(&app, USER).to_string(),
            repay_amount: Uint128::new(1000),
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_record_bad_debt_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::RecordBadDebt {
            position_id: Uint128::one(), position_owner: get_addr(&app, USER).to_string(),
            bad_debt_amount: Uint128::new(1000),
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_callback_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::Callback(
            CallbackMsg::ClosePositionCallback {
                position_id: Uint128::one(), position_owner: get_addr(&app, USER).to_string(),
            }
        ), &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // RecordBadDebt Tests
    // ========================================================================

    #[test]
    fn test_record_bad_debt_valid() {
        let mut app = mock_app();
        let (debt_addr, _, _, liquidation) = instantiate_debt_contract(&mut app);
        let owner = get_addr(&app, USER);
        app.execute_contract(liquidation.clone(), debt_addr.clone(), &ExecuteMsg::RecordBadDebt {
            position_id: Uint128::one(), position_owner: owner.to_string(),
            bad_debt_amount: Uint128::new(5_000_000),
        }, &[]).unwrap();
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::new(5_000_000));
    }

    #[test]
    fn test_record_bad_debt_accumulates() {
        let mut app = mock_app();
        let (debt_addr, _, _, liquidation) = instantiate_debt_contract(&mut app);
        let owner = get_addr(&app, USER);
        app.execute_contract(liquidation.clone(), debt_addr.clone(), &ExecuteMsg::RecordBadDebt {
            position_id: Uint128::one(), position_owner: owner.to_string(), bad_debt_amount: Uint128::new(3_000_000),
        }, &[]).unwrap();
        app.execute_contract(liquidation.clone(), debt_addr.clone(), &ExecuteMsg::RecordBadDebt {
            position_id: Uint128::one(), position_owner: owner.to_string(), bad_debt_amount: Uint128::new(2_000_000),
        }, &[]).unwrap();
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::new(5_000_000));
    }

    // ========================================================================
    // SetAffiliate Tests
    // ========================================================================

    #[test]
    fn test_set_affiliate_by_admin() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let affiliate = get_addr(&app, "affiliate");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetAffiliate {
            position_id: Uint128::one(), affiliate_address: affiliate.to_string(),
            affiliate_fee: Decimal::percent(3), label: Some("campaign_1".to_string()),
        }, &[]).unwrap();
        let data: Option<membrane::types::AffiliateData> = app.wrap()
            .query_wasm_smart(debt_addr.clone(), &QueryMsg::GetAffiliates { position_id: Uint128::one() })
            .unwrap();
        assert!(data.is_some());
        let d = data.unwrap();
        assert_eq!(d.affiliate_address, affiliate.to_string());
        assert_eq!(d.affiliate_fee, Decimal::percent(3));
        assert_eq!(d.label, Some("campaign_1".to_string()));
    }

    #[test]
    fn test_set_affiliate_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let affiliate = get_addr(&app, "affiliate");
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::SetAffiliate {
            position_id: Uint128::one(), affiliate_address: affiliate.to_string(),
            affiliate_fee: Decimal::percent(3), label: None,
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_set_affiliate_exceeds_max_fee() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let affiliate = get_addr(&app, "affiliate");
        let err = app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetAffiliate {
            position_id: Uint128::one(), affiliate_address: affiliate.to_string(),
            affiliate_fee: Decimal::percent(10), label: None, // Max is 5%
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Affiliate fee exceeds max"));
    }

    // ========================================================================
    // CheckAndClearDebtDelta Tests
    // ========================================================================

    #[test]
    fn test_check_and_clear_debt_delta_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::CheckAndClearDebtDelta {
            position_owner: get_addr(&app, USER).to_string(), position_id: Uint128::one(),
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_check_and_clear_debt_delta_by_points_contract() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        let points_addr = get_addr(&app, POINTS_CONTRACT);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        let resp = app.execute_contract(points_addr, debt_addr.clone(), &ExecuteMsg::CheckAndClearDebtDelta {
            position_owner: get_addr(&app, USER).to_string(), position_id: Uint128::one(),
        }, &[]).unwrap();
        let method_attr = resp.events.iter().flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "method" && a.value == "check_and_clear_debt_delta");
        assert!(method_attr.is_some());
    }

    // ========================================================================
    // Query Tests
    // ========================================================================

    #[test]
    fn test_query_config() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        let config = query_config(&app, &debt_addr);
        assert_eq!(config.owner, admin);
        assert_eq!(config.collateral_contract, Some(collateral));
        assert_eq!(config.credit_twap_timeframe, 60);
    }

    #[test]
    fn test_query_basket() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.basket_id, Uint128::one());
        assert_eq!(basket.pending_bad_debt, Uint128::zero());
        match basket.credit_asset.info {
            AssetInfo::NativeToken { denom } => assert_eq!(denom, CDT_DENOM),
            _ => panic!("Expected NativeToken credit asset"),
        }
    }

    #[test]
    fn test_query_debt_positions_empty() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let resp = query_debt_positions(&app, &debt_addr, USER);
        assert!(resp.positions.is_empty());
    }

    #[test]
    fn test_query_debt_positions_with_data() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::new(2), USER);
        let resp = query_debt_positions(&app, &debt_addr, USER);
        assert_eq!(resp.positions.len(), 2);
    }

    #[test]
    fn test_query_debt_position_by_user_info() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::new(2), USER);
        let resp = query_debt_position_by_id(&app, &debt_addr, USER, Uint128::new(2));
        assert_eq!(resp.positions.len(), 1);
        assert_eq!(resp.positions[0].position_id, Uint128::new(2));
    }

    #[test]
    fn test_query_credit_rate() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let price = query_credit_rate(&app, &debt_addr);
        assert_eq!(price.price, Decimal::one());
    }

    #[test]
    fn test_query_frozen() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        assert_eq!(query_frozen(&app, &debt_addr), false);
        app.execute_contract(admin, debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), true);
    }

    #[test]
    fn test_query_rates_initially_none() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let rates = query_rates(&app, &debt_addr);
        assert!(rates.is_none(), "Rates should not be initialized on instantiate");
    }

    #[test]
    fn test_query_active_deployment_venues_empty() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let venues: Vec<String> = app.wrap().query_wasm_smart(debt_addr.clone(),
            &QueryMsg::GetActiveDeploymentVenues { venue: None, start_after: None, limit: None }).unwrap();
        assert!(venues.is_empty());
    }

    #[test]
    fn test_query_historical_interest_rates_empty() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let history: Vec<crate::state::RateTimestamp> = app.wrap().query_wasm_smart(debt_addr.clone(),
            &QueryMsg::GetHistoricalInterestRates { asset: "uosmo".to_string() }).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_query_liquidation_stats_empty() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let stats: Vec<crate::state::LiquidationEvent> = app.wrap().query_wasm_smart(debt_addr.clone(),
            &QueryMsg::GetLiquidationStats { start_after: None, limit: None }).unwrap();
        assert!(stats.is_empty());
    }


    #[test]
    fn test_frozen_blocks_increase_debt() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        let err = app.execute_contract(user.clone(), debt_addr.clone(), &ExecuteMsg::IncreaseDebt {
            position_id: Uint128::one(), amount: Some(Uint128::new(5_000_000_000)),
            LTV: None, mint_to_addr: None, deployment_intent: None,
            debt_split: None, rollover_updates: None, peg_debt: None,
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    #[test]
    fn test_frozen_blocks_close_position() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        let err = app.execute_contract(user.clone(), debt_addr.clone(), &ExecuteMsg::ClosePosition {
            position_id: Uint128::one(), close_percentage: None,
            max_spread: Decimal::percent(10), send_to: None,
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    #[test]
    fn test_frozen_toggle_cycle() {
        let mut app = mock_app();
        let (debt_addr, admin, collateral, _) = instantiate_debt_contract(&mut app);
        assert_eq!(query_frozen(&app, &debt_addr), false);
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), true);
        app.execute_contract(collateral.clone(), debt_addr.clone(), &ExecuteMsg::SetFrozen { frozen: false }, &[]).unwrap();
        assert_eq!(query_frozen(&app, &debt_addr), false);
    }

    #[test]
    fn test_ownership_transfer() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let new_owner = get_addr(&app, "new_owner");
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: Some(new_owner.to_string()), collateral_contract: None, liquidation_contract: None,
            staking_contract: None, chain_proxy: None, debt_auction: None, oracle_contract: None,
            liquidity_contract: None, discounts_contract: None, revenue_distributor: None,
            transmuter_addr: None, points_contract: None, liq_fee: None, credit_twap_timeframe: None,
            cpc_multiplier: None, debt_minimum: None, base_debt_cap_multiplier: None,
            rate_slope_multiplier: None, affiliate_fee_max: None, skip_credit_price_accrual: None,
            liquidation_stat_limit: None, irm_config: None, acquisition_contract: None,
        }), &[]).unwrap();
        let err = app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::UpdateConfig(UpdateConfig {
            owner: None, collateral_contract: None, liquidation_contract: None,
            staking_contract: None, chain_proxy: None, debt_auction: None, oracle_contract: None,
            liquidity_contract: None, discounts_contract: None, revenue_distributor: None,
            transmuter_addr: None, points_contract: None, liq_fee: Some(Decimal::percent(50)),
            credit_twap_timeframe: None, cpc_multiplier: None, debt_minimum: None,
            base_debt_cap_multiplier: None, rate_slope_multiplier: None, affiliate_fee_max: None,
            skip_credit_price_accrual: None, liquidation_stat_limit: None, irm_config: None,
            acquisition_contract: None,
        }), &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_take_revenue_unauthorized() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let random = get_addr(&app, USER);
        let err = app.execute_contract(random, debt_addr.clone(), &ExecuteMsg::TakeRevenue {}, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_take_revenue_disabled() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        let err = app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::TakeRevenue {}, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("Revenue distribution is disabled"));
    }

    #[test]
    fn test_take_revenue_no_pending() {
        let mut app = mock_app();
        let (debt_addr, admin, _, _) = instantiate_debt_contract(&mut app);
        app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::EditBasket(EditBasketDebt {
            liq_queue: None, credit_pool_infos: None, credit_asset_twap_price_source: None,
            base_interest_rate: None, negative_rates: None, cpc_margin_of_error: None,
            distribute_revenue: Some(true), take_revenue: None,
        }), &[]).unwrap();
        let err = app.execute_contract(admin.clone(), debt_addr.clone(), &ExecuteMsg::TakeRevenue {}, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("No pending revenue"));
    }

    #[test]
    fn test_fulfill_bad_debt_no_funds() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        let err = app.execute_contract(user, debt_addr.clone(), &ExecuteMsg::FulfillBadDebt {}, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("No CDT sent to fulfill bad debt"));
    }

    #[test]
    fn test_fulfill_bad_debt_reduces_pending() {
        let mut app = mock_app();
        // Use custom instantiate with chain_proxy=None so burn msg is not sent
        // (no mock chain_proxy contract available in test)
        let debt_code_id = app.store_code(debt_contract());
        let admin = get_addr(&app, ADMIN);
        let mut msg = default_instantiate_msg(&app);
        msg.chain_proxy = None;
        let debt_addr = app.instantiate_contract(debt_code_id, admin.clone(), &msg, &[], "debt", None).unwrap();
        let liquidation = get_addr(&app, LIQUIDATION_CONTRACT);
        let user = get_addr(&app, USER);
        app.execute_contract(liquidation.clone(), debt_addr.clone(), &ExecuteMsg::RecordBadDebt {
            position_id: Uint128::one(), position_owner: user.to_string(),
            bad_debt_amount: Uint128::new(10_000_000),
        }, &[]).unwrap();
        let resp = app.execute_contract(user.clone(), debt_addr.clone(), &ExecuteMsg::FulfillBadDebt {},
            &[coin(5_000_000u128, CDT_DENOM)]).unwrap();
        let applied = resp.events.iter().flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "applied");
        assert!(applied.is_some());
        assert_eq!(applied.unwrap().value, "5000000");
        let basket = query_basket(&app, &debt_addr);
        assert_eq!(basket.pending_bad_debt, Uint128::new(5_000_000));
    }

    #[test]
    fn test_query_position_nonexistent_returns_empty() {
        let mut app = mock_app();
        let (debt_addr, _, collateral, _) = instantiate_debt_contract(&mut app);
        create_position_via_notify(&mut app, &debt_addr, &collateral, Uint128::one(), USER);
        let resp = query_debt_position_by_id(&app, &debt_addr, USER, Uint128::new(999));
        assert!(resp.positions.is_empty());
    }

    #[test]
    fn test_query_affiliates_none() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let data: Option<membrane::types::AffiliateData> = app.wrap()
            .query_wasm_smart(debt_addr.clone(), &QueryMsg::GetAffiliates { position_id: Uint128::new(999) })
            .unwrap();
        assert!(data.is_none());
    }

    #[test]
    fn test_increase_debt_no_position() {
        let mut app = mock_app();
        let (debt_addr, _, _, _) = instantiate_debt_contract(&mut app);
        let user = get_addr(&app, USER);
        let err = app.execute_contract(user, debt_addr.clone(), &ExecuteMsg::IncreaseDebt {
            position_id: Uint128::one(), amount: Some(Uint128::new(5_000_000_000)),
            LTV: None, mint_to_addr: None, deployment_intent: None,
            debt_split: None, rollover_updates: None, peg_debt: None,
        }, &[]).unwrap_err();
        assert!(err.root_cause().to_string().contains("No user positions") ||
                err.root_cause().to_string().contains("not found"));
    }

}

