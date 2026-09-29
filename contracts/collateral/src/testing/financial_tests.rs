#![allow(non_snake_case)]

pub mod tests {

    use membrane::collateral::*;
    use membrane::oracle::PriceResponse;
    use membrane::types::{cAsset, Asset, AssetInfo, SupplyCap, MultiAssetSupplyCap, UserInfo};

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult,
        Uint128,
    };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    fn get_user_addr(app: &App) -> Addr { app.api().addr_make(USER) }
    fn get_admin_addr(app: &App) -> Addr { app.api().addr_make(ADMIN) }

    pub fn collateral_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute, crate::contract::instantiate, crate::contract::query,
        ).with_reply(crate::contract::reply);
        Box::new(contract)
    }

    #[cw_serde] pub enum OracleMockExecuteMsg {}
    #[cw_serde] pub struct OracleMockInstantiateMsg {}
    #[cw_serde]
    pub enum OracleMockQueryMsg {
        Price { asset_info: AssetInfo, twap_timeframe: u64, oracle_time_limit: u64, basket_id: Option<Uint128> },
        Prices { asset_infos: Vec<AssetInfo>, twap_timeframe: u64, oracle_time_limit: u64 },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: OracleMockExecuteMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, _info, _msg: OracleMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, msg: OracleMockQueryMsg| -> StdResult<Binary> {
                match msg {
                    OracleMockQueryMsg::Price { .. } => to_json_binary(&PriceResponse { prices: vec![], price: Decimal::one(), decimals: 6 }),
                    OracleMockQueryMsg::Prices { asset_infos, .. } => {
                        let r: Vec<PriceResponse> = asset_infos.iter().map(|_| PriceResponse { prices: vec![], price: Decimal::one(), decimals: 6 }).collect();
                        to_json_binary(&r)
                    }
                }
            },
        );
        Box::new(contract)
    }

    #[cw_serde] pub struct LTVDiscoMockInstantiateMsg {}

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        use membrane::ltv_disco::{ExecuteMsg as LTVDiscoExecuteMsg, QueryMsg as LTVDiscoQueryMsg};
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: LTVDiscoExecuteMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, _info, _msg: LTVDiscoMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, msg: LTVDiscoQueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDiscoQueryMsg::CanHandleBadDebt { .. } => to_json_binary(&false),
                    _ => to_json_binary(&true),
                }
            },
        );
        Box::new(contract)
    }

    #[cw_serde]
    pub enum DebtMockExecuteMsg {
        NotifyDeposit { position_id: Uint128, position_owner: String, affiliate_address: Option<String> },
        AccrueAndValidate { position_owner: String, position_id: Uint128, remaining_collateral_value: Decimal, avg_max_borrow_ltv: Decimal, avg_max_ltv: Decimal },
    }
    #[cw_serde] pub struct DebtMockInstantiateMsg {}
    #[cw_serde] pub enum DebtMockQueryMsg {}

    pub fn debt_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: DebtMockExecuteMsg| -> StdResult<Response> {
                match msg {
                    DebtMockExecuteMsg::NotifyDeposit { .. } => {
                        Ok(Response::new().add_attribute("method", "notify_deposit"))
                    }
                    DebtMockExecuteMsg::AccrueAndValidate { .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "accrue_and_validate")
                            .add_attribute("total_debt", "0"))
                    }
                }
            },
            |_deps, _env, _info, _msg: DebtMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, _msg: DebtMockQueryMsg| -> StdResult<Binary> { Ok(Binary::default()) },
        );
        Box::new(contract)
    }

    #[cw_serde] pub enum LiqMockExecuteMsg {}
    #[cw_serde] pub struct LiqMockInstantiateMsg {}
    #[cw_serde] pub enum LiqMockQueryMsg {}

    pub fn liquidation_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: LiqMockExecuteMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, _info, _msg: LiqMockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_deps, _env, _msg: LiqMockQueryMsg| -> StdResult<Binary> { Ok(Binary::default()) },
        );
        Box::new(contract)
    }

    fn make_casset(denom: &str, borrow_ltv: u64, max_ltv: u64) -> cAsset {
        cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken { denom: denom.to_string() },
                amount: Uint128::zero(),
            },
            max_borrow_LTV: Decimal::percent(borrow_ltv),
            max_LTV: Decimal::percent(max_ltv),
            rate_index: Decimal::one(),
            peg_rate_index: Decimal::one(),
            pool_info: None,
            force_redemptions: None,
            max_threshold_to_delay: Decimal::percent(15),
        }
    }

    fn make_supply_cap(denom: &str) -> SupplyCap {
        SupplyCap {
            asset_info: AssetInfo::NativeToken { denom: denom.to_string() },
            current_supply: Uint128::zero(),
            debt_total: Uint128::zero(),
            supply_cap_ratio: Decimal::percent(100),
            lp: false,
            stability_pool_ratio_for_debt_cap: None,
        }
    }

    struct TestContracts {
        collateral_addr: Addr,
        #[allow(dead_code)]
        oracle_addr: Addr,
        #[allow(dead_code)]
        ltv_disco_addr: Addr,
        debt_addr: Addr,
        liq_addr: Addr,
    }

    fn proper_instantiate() -> (App, TestContracts) {
        let mut app = AppBuilder::default().build(|router, api, storage| {
            let user_addr = api.addr_make(USER);
            let admin_addr = api.addr_make(ADMIN);
            let bigger_bank_addr = api.addr_make("bigger_bank");
            router.bank.init_balance(storage, &user_addr, vec![
                coin(100_000_000_000, "uatom"), coin(100_000_000_000, "uosmo"), coin(100_000_000_000, "uusdc"),
            ]).unwrap();
            router.bank.init_balance(storage, &admin_addr, vec![
                coin(100_000_000_000, "uatom"), coin(100_000_000_000, "uosmo"), coin(100_000_000_000, "uusdc"),
            ]).unwrap();
            router.bank.init_balance(storage, &bigger_bank_addr, vec![
                coin(1_000_000_000_000, "uatom"), coin(1_000_000_000_000, "uosmo"), coin(1_000_000_000_000, "uusdc"),
            ]).unwrap();
        });

        let admin = get_admin_addr(&app);
        let oracle_code_id = app.store_code(oracle_contract());
        let ltv_disco_code_id = app.store_code(ltv_disco_contract());
        let debt_code_id = app.store_code(debt_contract());
        let liq_code_id = app.store_code(liquidation_contract());
        let collateral_code_id = app.store_code(collateral_contract());

        let oracle_addr = app.instantiate_contract(oracle_code_id, admin.clone(), &OracleMockInstantiateMsg {}, &[], "oracle", None).unwrap();
        let ltv_disco_addr = app.instantiate_contract(ltv_disco_code_id, admin.clone(), &LTVDiscoMockInstantiateMsg {}, &[], "ltv_disco", None).unwrap();
        let debt_addr = app.instantiate_contract(debt_code_id, admin.clone(), &DebtMockInstantiateMsg {}, &[], "debt", None).unwrap();
        let liq_addr = app.instantiate_contract(liq_code_id, admin.clone(), &LiqMockInstantiateMsg {}, &[], "liquidation", None).unwrap();

        let collateral_addr = app.instantiate_contract(
            collateral_code_id, admin.clone(),
            &InstantiateMsg {
                owner: Some(admin.to_string()),
                oracle_contract: Some(oracle_addr.to_string()),
                chain_proxy: None,
                ltv_disco: ltv_disco_addr.to_string(),
                debt_contract: Some(debt_addr.to_string()),
                liquidation_contract: Some(liq_addr.to_string()),
                collateral_twap_timeframe: 60u64,
                oracle_time_limit: 300u64,
                create_basket: CreateBasket {
                    basket_id: Uint128::one(),
                    collateral_types: vec![make_casset("uatom", 50, 60), make_casset("uosmo", 40, 50)],
                    collateral_supply_caps: vec![make_supply_cap("uatom"), make_supply_cap("uosmo")],
                    multi_asset_supply_caps: vec![],
                },
                ltv_upward_kp: None,
                ltv_downward_period: None,
                ltv_downward_decay_rate: None,
                ltv_borrow_distance: None,
            },
            &[], "collateral", None,
        ).unwrap();

        (app, TestContracts { collateral_addr, oracle_addr, ltv_disco_addr, debt_addr, liq_addr })
    }

    fn proper_instantiate_three_assets() -> (App, TestContracts) {
        let mut app = AppBuilder::default().build(|router, api, storage| {
            let user_addr = api.addr_make(USER);
            let admin_addr = api.addr_make(ADMIN);
            let bigger_bank_addr = api.addr_make("bigger_bank");
            router.bank.init_balance(storage, &user_addr, vec![
                coin(100_000_000_000, "uatom"), coin(100_000_000_000, "uosmo"), coin(100_000_000_000, "uusdc"),
            ]).unwrap();
            router.bank.init_balance(storage, &admin_addr, vec![
                coin(100_000_000_000, "uatom"), coin(100_000_000_000, "uosmo"), coin(100_000_000_000, "uusdc"),
            ]).unwrap();
            router.bank.init_balance(storage, &bigger_bank_addr, vec![
                coin(1_000_000_000_000, "uatom"), coin(1_000_000_000_000, "uosmo"), coin(1_000_000_000_000, "uusdc"),
            ]).unwrap();
        });

        let admin = get_admin_addr(&app);
        let oracle_code_id = app.store_code(oracle_contract());
        let ltv_disco_code_id = app.store_code(ltv_disco_contract());
        let debt_code_id = app.store_code(debt_contract());
        let liq_code_id = app.store_code(liquidation_contract());
        let collateral_code_id = app.store_code(collateral_contract());

        let oracle_addr = app.instantiate_contract(oracle_code_id, admin.clone(), &OracleMockInstantiateMsg {}, &[], "oracle", None).unwrap();
        let ltv_disco_addr = app.instantiate_contract(ltv_disco_code_id, admin.clone(), &LTVDiscoMockInstantiateMsg {}, &[], "ltv_disco", None).unwrap();
        let debt_addr = app.instantiate_contract(debt_code_id, admin.clone(), &DebtMockInstantiateMsg {}, &[], "debt", None).unwrap();
        let liq_addr = app.instantiate_contract(liq_code_id, admin.clone(), &LiqMockInstantiateMsg {}, &[], "liquidation", None).unwrap();

        let collateral_addr = app.instantiate_contract(
            collateral_code_id, admin.clone(),
            &InstantiateMsg {
                owner: Some(admin.to_string()),
                oracle_contract: Some(oracle_addr.to_string()),
                chain_proxy: None,
                ltv_disco: ltv_disco_addr.to_string(),
                debt_contract: Some(debt_addr.to_string()),
                liquidation_contract: Some(liq_addr.to_string()),
                collateral_twap_timeframe: 60u64,
                oracle_time_limit: 300u64,
                create_basket: CreateBasket {
                    basket_id: Uint128::one(),
                    collateral_types: vec![
                        make_casset("uatom", 50, 60),
                        make_casset("uosmo", 40, 50),
                        make_casset("uusdc", 60, 70),
                    ],
                    collateral_supply_caps: vec![
                        make_supply_cap("uatom"),
                        make_supply_cap("uosmo"),
                        make_supply_cap("uusdc"),
                    ],
                    multi_asset_supply_caps: vec![],
                },
                ltv_upward_kp: None,
                ltv_downward_period: None,
                ltv_downward_decay_rate: None,
                ltv_borrow_distance: None,
            },
            &[], "collateral", None,
        ).unwrap();

        (app, TestContracts { collateral_addr, oracle_addr, ltv_disco_addr, debt_addr, liq_addr })
    }

    fn deposit_and_get_position_id(app: &mut App, contracts: &TestContracts, amount: u128, denom: &str) -> Uint128 {
        let user = get_user_addr(app);
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(amount, denom)],
        ).unwrap();
        let positions_resp: CollateralPositionsResponse = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCollateralPositions { start_after: None, limit: None, user_info: None, user: Some(user.to_string()) },
        ).unwrap();
        positions_resp.positions.last().unwrap().position_id
    }

    // ========================================================================
    // A. WITHDRAW FLOW TESTS
    // ========================================================================

    #[test]
    fn test_withdraw_success_partial() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(bigger_bank, contracts.collateral_addr.clone(), &[coin(1_000_000, "uatom")]).unwrap();
        let res = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: pid,
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(400_000) }],
                send_to: None,
            },
            &[],
        ).unwrap();
        assert!(res.events.iter().any(|e| e.attributes.iter().any(|a| a.key == "method" && a.value == "withdraw")));
        let positions_resp: CollateralPositionsResponse = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCollateralPositions { start_after: None, limit: None, user_info: None, user: Some(user.to_string()) },
        ).unwrap();
        assert_eq!(positions_resp.positions[0].collateral_assets[0].asset.amount, Uint128::new(600_000));
    }

    #[test]
    fn test_withdraw_when_frozen_fails() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        app.execute_contract(admin.clone(), contracts.collateral_addr.clone(), &ExecuteMsg::SetFrozen { frozen: true }, &[]).unwrap();
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: pid,
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(500_000) }],
                send_to: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    #[test]
    fn test_withdraw_nonexistent_position_fails() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: Uint128::new(99),
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(500_000) }],
                send_to: None,
            },
            &[],
        ).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("No user positions")
            || err.root_cause().to_string().contains("does not exist")
        );
    }

    #[test]
    fn test_withdraw_excessive_amount_fails() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: pid,
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(2_000_000) }],
                send_to: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("exceeds") || err.root_cause().to_string().contains("Excessive"));
    }

    #[test]
    fn test_withdraw_send_to_different_address() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let recipient = app.api().addr_make("recipient");
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(bigger_bank, contracts.collateral_addr.clone(), &[coin(1_000_000, "uatom")]).unwrap();
        let res = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: pid,
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(300_000) }],
                send_to: Some(recipient.to_string()),
            },
            &[],
        ).unwrap();
        assert!(res.events.iter().any(|e| e.attributes.iter().any(|a| a.key == "send_to" && a.value == recipient.to_string())));
    }

    #[test]
    fn test_withdraw_full_removal_removes_asset() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")],
        ).unwrap();
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(bigger_bank, contracts.collateral_addr.clone(), &[coin(1_000_000, "uatom")]).unwrap();
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: Uint128::new(1),
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(1_000_000) }],
                send_to: None,
            },
            &[],
        ).unwrap();
        let positions_resp: CollateralPositionsResponse = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCollateralPositions { start_after: None, limit: None, user_info: None, user: Some(user.to_string()) },
        ).unwrap();
        assert_eq!(positions_resp.positions[0].collateral_assets.len(), 1);
        assert_eq!(positions_resp.positions[0].collateral_assets[0].asset.info.to_string(), "uosmo");
    }

    #[test]
    fn test_withdraw_asset_not_in_position_fails() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: pid,
                assets: vec![Asset { info: AssetInfo::NativeToken { denom: "uosmo".to_string() }, amount: Uint128::new(500_000) }],
                send_to: None,
            },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Invalid collateral") || err.root_cause().to_string().contains("collateral"));
    }

    // ========================================================================
    // B. REFRESH AND ASSESS TESTS
    // ========================================================================

    #[test]
    fn test_refresh_and_assess_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: pid },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_refresh_and_assess_from_debt_contract() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let res = app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: pid },
            &[],
        ).unwrap();
        assert!(res.events.iter().any(|e| e.attributes.iter().any(|a| a.key == "method" && a.value == "refresh_and_assess")));
        assert!(res.events.iter().any(|e| e.attributes.iter().any(|a| a.key == "assessment")));
    }

    #[test]
    fn test_refresh_and_assess_from_liquidation_contract() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        let res = app.execute_contract(
            contracts.liq_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: pid },
            &[],
        ).unwrap();
        assert!(res.events.iter().any(|e| e.attributes.iter().any(|a| a.key == "method" && a.value == "refresh_and_assess")));
    }

    #[test]
    fn test_refresh_and_assess_multi_collateral_ltvs() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(1_000_000, "uatom"), coin(1_000_000, "uosmo")],
        ).unwrap();
        let res = app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: Uint128::new(1) },
            &[],
        ).unwrap();
        let assessment_attr = res.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "assessment")
            .unwrap();
        let binary = cosmwasm_std::Binary::from_base64(&assessment_attr.value).unwrap();
        let assessment: CollateralAssessmentResponse = cosmwasm_std::from_json(binary).unwrap();
        // Equal amounts at : borrow LTV = (50+40)/2 = 45%, max LTV = (60+50)/2 = 55%
        assert_eq!(assessment.avg_max_borrow_ltv, Decimal::percent(45));
        assert_eq!(assessment.avg_max_ltv, Decimal::percent(55));
    }


    // ========================================================================
    // C. CIRCUIT BREAKER TESTS
    // ========================================================================

    #[test]
    fn test_circuit_breaker_query_uninitialized() {
        let (app, contracts) = proper_instantiate();
        // Query circuit breaker for an asset that has no breaker set
        let status: Vec<(String, Option<crate::state::AssetCircuitBreaker>)> = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCircuitBreakerStatus { assets: vec!["uatom".to_string()] },
        ).unwrap();
        assert_eq!(status.len(), 1);
        assert!(status[0].1.is_none(), "Should have no circuit breaker initially");
    }

    #[test]
    fn test_circuit_breaker_query_multi_asset() {
        let (app, contracts) = proper_instantiate();
        let status: Vec<(String, Option<crate::state::AssetCircuitBreaker>)> = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCircuitBreakerStatus { assets: vec!["uatom".to_string(), "uosmo".to_string()] },
        ).unwrap();
        assert_eq!(status.len(), 2);
        assert!(status[0].1.is_none());
        assert!(status[1].1.is_none());
    }

    #[test]
    fn test_volatility_window_query_no_data() {
        let (app, contracts) = proper_instantiate();
        let volatility: Vec<(String, bool)> = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::CheckVolatilityWindow { assets: vec!["uatom".to_string()] },
        ).unwrap();
        assert_eq!(volatility.len(), 1);
        assert_eq!(volatility[0].0, "uatom");
        assert!(!volatility[0].1, "Should not be volatile with no data");
    }

    #[test]
    fn test_volatility_window_after_refresh() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");
        // RefreshAndAssess populates VOLATILITY
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: pid },
            &[],
        ).unwrap();
        let volatility: Vec<(String, bool)> = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::CheckVolatilityWindow { assets: vec!["uatom".to_string()] },
        ).unwrap();
        assert_eq!(volatility.len(), 1);
        // With stable mock oracle, volatility should be low (not volatile)
        assert!(!volatility[0].1, "Stable price should not trigger volatility");
    }

    // ========================================================================
    // D. SUPPLY CAP TESTS
    // ========================================================================

    #[test]
    fn test_supply_cap_activate() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");

        // Activate supply caps (only callable by debt contract)
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::ActivateSupplyCaps { position_id: pid, position_owner: user.to_string() },
            &[],
        ).unwrap();

        let basket: BasketCollateral = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetBasket {},
        ).unwrap();
        let atom_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uatom").unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::new(1_000_000));
    }

    #[test]
    fn test_supply_cap_activate_then_deactivate() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");

        // Activate
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::ActivateSupplyCaps { position_id: pid, position_owner: user.to_string() },
            &[],
        ).unwrap();

        let basket: BasketCollateral = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetBasket {},
        ).unwrap();
        let atom_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uatom").unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::new(1_000_000));

        // Deactivate
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::DeactivateSupplyCaps { position_id: pid, position_owner: user.to_string() },
            &[],
        ).unwrap();

        let basket: BasketCollateral = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetBasket {},
        ).unwrap();
        let atom_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uatom").unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::zero());
    }

    #[test]
    fn test_supply_cap_multi_asset() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Deposit both assets in one position
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")],
        ).unwrap();

        // Activate supply caps
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::ActivateSupplyCaps { position_id: Uint128::new(1), position_owner: user.to_string() },
            &[],
        ).unwrap();

        let basket: BasketCollateral = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetBasket {},
        ).unwrap();
        let atom_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uatom").unwrap();
        let osmo_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uosmo").unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::new(1_000_000));
        assert_eq!(osmo_cap.current_supply, Uint128::new(2_000_000));
    }

    #[test]
    fn test_supply_cap_ratio_edit() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        // Reduce uatom supply_cap_ratio to 50%
        app.execute_contract(
            admin.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken { denom: "uatom".to_string() },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(50),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                multi_asset_supply_caps: None,
            }),
            &[],
        ).unwrap();

        let basket: BasketCollateral = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetBasket {},
        ).unwrap();
        let atom_cap = basket.collateral_supply_caps.iter().find(|cap| cap.asset_info.to_string() == "uatom").unwrap();
        assert_eq!(atom_cap.supply_cap_ratio, Decimal::percent(50));
    }

    // ========================================================================
    // E. EDGE CASE TESTS
    // ========================================================================

    #[test]
    fn test_callback_only_self() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // WithdrawCallback can only be called by the contract itself
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Callback(CallbackMsg::WithdrawCallback { position_id: Uint128::one(), position_owner: "user".to_string() }),
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_rate_assurance_only_self() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // CollateralRateAssurance can only be called by the contract itself
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::CollateralRateAssurance { collateral_denoms: None },
            &[],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_query_position_value_after_refresh() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let pid = deposit_and_get_position_id(&mut app, &contracts, 1_000_000, "uatom");

        // RefreshAndAssess to populate STORED_PRICES
        app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: pid },
            &[],
        ).unwrap();

        let value_resp: PositionValueResponse = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetPositionValue {
                position_owner: user.to_string(),
                position_id: pid,
            },
        ).unwrap();
        assert!(value_resp.total_value > Decimal::zero(), "Position value should be non-zero");
    }

    #[test]
    fn test_max_positions_per_user() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create 9 positions (MAX_POSITIONS_PER_USER = 9)
        for _i in 0..9 {
            app.execute_contract(
                user.clone(), contracts.collateral_addr.clone(),
                &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
                &[coin(100_000, "uatom")],
            ).unwrap();
        }

        // 10th position should fail
        let err = app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(100_000, "uatom")],
        ).unwrap_err();
        assert!(err.root_cause().to_string().contains("Maximum positions") || err.root_cause().to_string().contains("MaxPositionsReached"));
    }

    #[test]
    fn test_three_asset_assess() {
        let (mut app, contracts) = proper_instantiate_three_assets();
        let user = get_user_addr(&app);

        // Deposit all three assets
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(1_000_000, "uatom"), coin(1_000_000, "uosmo"), coin(1_000_000, "uusdc")],
        ).unwrap();

        // RefreshAndAssess with three collateral types
        let res = app.execute_contract(
            contracts.debt_addr.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::RefreshAndAssess { position_owner: user.to_string(), position_id: Uint128::new(1) },
            &[],
        ).unwrap();

        let assessment_attr = res.events.iter()
            .flat_map(|e| e.attributes.iter())
            .find(|a| a.key == "assessment")
            .unwrap();
        let binary = cosmwasm_std::Binary::from_base64(&assessment_attr.value).unwrap();
        let assessment: CollateralAssessmentResponse = cosmwasm_std::from_json(binary).unwrap();

        // Three assets: weighted avg borrow LTV = (50+40+60)/3 ≈ 50%, max LTV = (60+50+70)/3 ≈ 60%
        // Use tolerance for fixed-point rounding (thirds produce repeating decimals)
        let tolerance = Decimal::from_ratio(1u128, 1_000_000_000u128);
        assert!(assessment.avg_max_borrow_ltv.abs_diff(Decimal::percent(50)) < tolerance);
        assert!(assessment.avg_max_ltv.abs_diff(Decimal::percent(60)) < tolerance);
        assert_eq!(assessment.collateral_assets.len(), 3);
    }

    #[test]
    fn test_partial_multi_asset_withdraw() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Deposit both assets
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit { position_id: None, position_owner: None, affiliate_address: None },
            &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")],
        ).unwrap();

        // Fund the contract for withdrawal
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(bigger_bank, contracts.collateral_addr.clone(), &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")]).unwrap();

        // Partial withdraw of both assets
        app.execute_contract(
            user.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::Withdraw {
                position_id: Uint128::new(1),
                assets: vec![
                    Asset { info: AssetInfo::NativeToken { denom: "uatom".to_string() }, amount: Uint128::new(200_000) },
                    Asset { info: AssetInfo::NativeToken { denom: "uosmo".to_string() }, amount: Uint128::new(500_000) },
                ],
                send_to: None,
            },
            &[],
        ).unwrap();

        // Verify remaining amounts
        let positions_resp: CollateralPositionsResponse = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetCollateralPositions { start_after: None, limit: None, user_info: None, user: Some(user.to_string()) },
        ).unwrap();
        let pos = &positions_resp.positions[0];
        let atom_ca = pos.collateral_assets.iter().find(|ca| ca.asset.info.to_string() == "uatom").unwrap();
        let osmo_ca = pos.collateral_assets.iter().find(|ca| ca.asset.info.to_string() == "uosmo").unwrap();
        assert_eq!(atom_ca.asset.amount, Uint128::new(800_000));
        assert_eq!(osmo_ca.asset.amount, Uint128::new(1_500_000));
    }

    #[test]
    fn test_historical_oracle_prices_empty() {
        let (app, contracts) = proper_instantiate();
        let history: Vec<crate::state::PriceTimestamp> = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(),
            &QueryMsg::GetHistoricalOraclePrices { asset: "uatom".to_string() },
        ).unwrap();
        assert!(history.is_empty(), "No historical prices before any refresh");
    }

    #[test]
    fn test_frozen_state_toggle() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        let frozen: bool = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetFrozen {},
        ).unwrap();
        assert!(!frozen);

        app.execute_contract(
            admin.clone(), contracts.collateral_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: true }, &[],
        ).unwrap();

        let frozen: bool = app.wrap().query_wasm_smart(
            contracts.collateral_addr.clone(), &QueryMsg::GetFrozen {},
        ).unwrap();
        assert!(frozen);
    }
}