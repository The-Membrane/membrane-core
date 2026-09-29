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

    // ========================================================================
    // Address Helpers
    // ========================================================================

    fn get_user_addr(app: &App) -> Addr {
        app.api().addr_make(USER)
    }

    fn get_admin_addr(app: &App) -> Addr {
        app.api().addr_make(ADMIN)
    }

    // ========================================================================
    // Collateral Contract Wrapper
    // ========================================================================

    pub fn collateral_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    // ========================================================================
    // Mock Oracle Contract
    // ========================================================================

    #[cw_serde]
    pub enum OracleMockExecuteMsg {}

    #[cw_serde]
    pub struct OracleMockInstantiateMsg {}

    #[cw_serde]
    pub enum OracleMockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            oracle_time_limit: u64,
            basket_id: Option<Uint128>,
        },
        Prices {
            asset_infos: Vec<AssetInfo>,
            twap_timeframe: u64,
            oracle_time_limit: u64,
        },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: OracleMockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _info, _msg: OracleMockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, msg: OracleMockQueryMsg| -> StdResult<Binary> {
                match msg {
                    OracleMockQueryMsg::Price {
                        asset_info: _,
                        twap_timeframe: _,
                        oracle_time_limit: _,
                        basket_id: _,
                    } => to_json_binary(&PriceResponse {
                        prices: vec![],
                        price: Decimal::one(),
                        decimals: 6,
                    }),
                    OracleMockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe: _,
                        oracle_time_limit: _,
                    } => {
                        // Return $1 price for each queried asset
                        let responses: Vec<PriceResponse> = asset_infos
                            .iter()
                            .map(|_| PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            })
                            .collect();
                        to_json_binary(&responses)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // ========================================================================
    // Mock LTV Disco Contract
    // ========================================================================

    #[cw_serde]
    pub struct LTVDiscoMockInstantiateMsg {}

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        use membrane::ltv_disco::{ExecuteMsg as LTVDiscoExecuteMsg, QueryMsg as LTVDiscoQueryMsg};
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: LTVDiscoExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _info, _msg: LTVDiscoMockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, msg: LTVDiscoQueryMsg| -> StdResult<Binary> {
                match msg {
                    LTVDiscoQueryMsg::CanHandleBadDebt { .. } => to_json_binary(&false),
                    _ => to_json_binary(&true),
                }
            },
        );
        Box::new(contract)
    }

    // ========================================================================
    // Mock Debt Contract
    // ========================================================================

    #[cw_serde]
    pub enum DebtMockExecuteMsg {
        NotifyDeposit {
            position_id: Uint128,
            position_owner: String,
            affiliate_address: Option<String>,
        },
        AccrueAndValidate {
            position_owner: String,
            position_id: Uint128,
            remaining_collateral_value: Decimal,
            avg_max_borrow_ltv: Decimal,
            avg_max_ltv: Decimal,
        },
    }

    #[cw_serde]
    pub struct DebtMockInstantiateMsg {}

    #[cw_serde]
    pub enum DebtMockQueryMsg {}

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
            |_deps, _env, _info, _msg: DebtMockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: DebtMockQueryMsg| -> StdResult<Binary> {
                Ok(Binary::default())
            },
        );
        Box::new(contract)
    }

    // ========================================================================
    // Mock Liquidation Contract
    // ========================================================================

    #[cw_serde]
    pub enum LiqMockExecuteMsg {}

    #[cw_serde]
    pub struct LiqMockInstantiateMsg {}

    #[cw_serde]
    pub enum LiqMockQueryMsg {}

    pub fn liquidation_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: LiqMockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _info, _msg: LiqMockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: LiqMockQueryMsg| -> StdResult<Binary> {
                Ok(Binary::default())
            },
        );
        Box::new(contract)
    }

    // ========================================================================
    // Test Setup
    // ========================================================================

    /// Helper to build a standard cAsset
    fn make_casset(denom: &str, borrow_ltv: u64, max_ltv: u64) -> cAsset {
        cAsset {
            asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: denom.to_string(),
                },
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

    /// Helper to build a SupplyCap for a native denom
    fn make_supply_cap(denom: &str) -> SupplyCap {
        SupplyCap {
            asset_info: AssetInfo::NativeToken {
                denom: denom.to_string(),
            },
            current_supply: Uint128::zero(),
            debt_total: Uint128::zero(),
            supply_cap_ratio: Decimal::percent(100),
            lp: false,
            stability_pool_ratio_for_debt_cap: None,
        }
    }

    struct TestContracts {
        collateral_addr: Addr,
        oracle_addr: Addr,
        ltv_disco_addr: Addr,
        debt_addr: Addr,
        liq_addr: Addr,
    }

    /// Full proper instantiate returning all contract addresses
    fn proper_instantiate() -> (App, TestContracts) {
        let mut app = AppBuilder::default().build(|router, api, storage| {
            let user_addr = api.addr_make(USER);
            let admin_addr = api.addr_make(ADMIN);
            let bigger_bank_addr = api.addr_make("bigger_bank");

            // Fund user
            router
                .bank
                .init_balance(
                    storage,
                    &user_addr,
                    vec![
                        coin(100_000_000_000, "uatom"),
                        coin(100_000_000_000, "uosmo"),
                        coin(100_000_000_000, "uusdc"),
                    ],
                )
                .unwrap();

            // Fund admin
            router
                .bank
                .init_balance(
                    storage,
                    &admin_addr,
                    vec![
                        coin(100_000_000_000, "uatom"),
                        coin(100_000_000_000, "uosmo"),
                        coin(100_000_000_000, "uusdc"),
                    ],
                )
                .unwrap();

            // Fund bigger_bank (for contract funding)
            router
                .bank
                .init_balance(
                    storage,
                    &bigger_bank_addr,
                    vec![
                        coin(1_000_000_000_000, "uatom"),
                        coin(1_000_000_000_000, "uosmo"),
                        coin(1_000_000_000_000, "uusdc"),
                    ],
                )
                .unwrap();
        });

        let admin = get_admin_addr(&app);

        // Store codes
        let oracle_code_id = app.store_code(oracle_contract());
        let ltv_disco_code_id = app.store_code(ltv_disco_contract());
        let debt_code_id = app.store_code(debt_contract());
        let liq_code_id = app.store_code(liquidation_contract());
        let collateral_code_id = app.store_code(collateral_contract());

        // Instantiate mock oracle
        let oracle_addr = app
            .instantiate_contract(
                oracle_code_id,
                admin.clone(),
                &OracleMockInstantiateMsg {},
                &[],
                "oracle",
                None,
            )
            .unwrap();

        // Instantiate mock LTV disco
        let ltv_disco_addr = app
            .instantiate_contract(
                ltv_disco_code_id,
                admin.clone(),
                &LTVDiscoMockInstantiateMsg {},
                &[],
                "ltv_disco",
                None,
            )
            .unwrap();

        // Instantiate mock debt contract
        let debt_addr = app
            .instantiate_contract(
                debt_code_id,
                admin.clone(),
                &DebtMockInstantiateMsg {},
                &[],
                "debt",
                None,
            )
            .unwrap();

        // Instantiate mock liquidation contract
        let liq_addr = app
            .instantiate_contract(
                liq_code_id,
                admin.clone(),
                &LiqMockInstantiateMsg {},
                &[],
                "liquidation",
                None,
            )
            .unwrap();

        // Instantiate collateral contract
        let collateral_addr = app
            .instantiate_contract(
                collateral_code_id,
                admin.clone(),
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
                        ],
                        collateral_supply_caps: vec![
                            make_supply_cap("uatom"),
                            make_supply_cap("uosmo"),
                        ],
                        multi_asset_supply_caps: vec![],
                    },
                    ltv_upward_kp: None,
                    ltv_downward_period: None,
                    ltv_downward_decay_rate: None,
                    ltv_borrow_distance: None,
                },
                &[],
                "collateral",
                None,
            )
            .unwrap();

        let contracts = TestContracts {
            collateral_addr,
            oracle_addr,
            ltv_disco_addr,
            debt_addr,
            liq_addr,
        };

        (app, contracts)
    }

    // ========================================================================
    // Test: Instantiation
    // ========================================================================

    #[test]
    fn test_proper_instantiate() {
        let (app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        // Query config
        let config: Config = app
            .wrap()
            .query_wasm_smart(contracts.collateral_addr.clone(), &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.owner, admin);
        assert_eq!(config.oracle_contract, Some(contracts.oracle_addr.clone()));
        assert_eq!(config.ltv_disco, contracts.ltv_disco_addr);
        assert_eq!(config.debt_contract, Some(contracts.debt_addr.clone()));
        assert_eq!(
            config.liquidation_contract,
            Some(contracts.liq_addr.clone())
        );
        assert_eq!(config.collateral_twap_timeframe, 60u64);
        assert_eq!(config.oracle_time_limit, 300u64);

        // Query basket
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        assert_eq!(basket.basket_id, Uint128::one());
        assert_eq!(basket.current_position_id, Uint128::zero());
        assert_eq!(basket.collateral_types.len(), 2);
        assert_eq!(basket.collateral_supply_caps.len(), 2);

        // Query frozen state
        let frozen: bool = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetFrozen {},
            )
            .unwrap();
        assert!(!frozen);
    }

    // ========================================================================
    // Test: Deposit - New Position
    // ========================================================================

    #[test]
    fn test_deposit_new_position() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Deposit uatom creating a new position (position_id = None)
        let deposit_msg = ExecuteMsg::Deposit {
            position_id: None,
            position_owner: None,
            affiliate_address: None,
        };
        let res = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &deposit_msg,
                &[coin(1_000_000, "uatom")],
            )
            .unwrap();

        // Check attributes
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "deposit")
        }));
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "action" && a.value == "create_position")
        }));

        // Query the position
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 1);
        assert_eq!(positions_resp.positions[0].position_id, Uint128::new(1));
        assert_eq!(
            positions_resp.positions[0].collateral_assets.len(),
            1
        );
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0].asset.amount,
            Uint128::new(1_000_000)
        );

        // Check basket position ID counter incremented
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        assert_eq!(basket.current_position_id, Uint128::new(1));
    }

    // ========================================================================
    // Test: Deposit - Existing Position
    // ========================================================================

    #[test]
    fn test_deposit_existing_position() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create position first
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Add more uatom to existing position (id = 1)
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: Some(Uint128::new(1)),
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(500_000, "uatom")],
        )
        .unwrap();

        // Query the position
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 1);
        // Amount should be 1_000_000 + 500_000 = 1_500_000
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0].asset.amount,
            Uint128::new(1_500_000)
        );
    }

    // ========================================================================
    // Test: Deposit - Multiple Collateral Types
    // ========================================================================

    #[test]
    fn test_deposit_multiple_collateral_types() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create position with uatom
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Add uosmo to same position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: Some(Uint128::new(1)),
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(2_000_000, "uosmo")],
        )
        .unwrap();

        // Query the position: should have both assets
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 1);
        assert_eq!(
            positions_resp.positions[0].collateral_assets.len(),
            2
        );
    }

    // ========================================================================
    // Test: Deposit - Invalid Collateral (not in basket)
    // ========================================================================

    #[test]
    fn test_deposit_invalid_collateral() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Fund user with an asset NOT in the basket
        let bigger_bank = app.api().addr_make("bigger_bank");
        app.send_tokens(bigger_bank, user.clone(), &[coin(1_000_000, "uusdc")])
            .unwrap();

        // Try to deposit uusdc which is not in the basket
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::Deposit {
                    position_id: None,
                    position_owner: None,
                    affiliate_address: None,
                },
                &[coin(1_000_000, "uusdc")],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Invalid collateral"));
    }

    // ========================================================================
    // Test: Deposit - Frozen State
    // ========================================================================

    #[test]
    fn test_deposit_when_frozen() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);
        let user = get_user_addr(&app);

        // Freeze the basket
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: true },
            &[],
        )
        .unwrap();

        // Try to deposit -- should fail
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::Deposit {
                    position_id: None,
                    position_owner: None,
                    affiliate_address: None,
                },
                &[coin(1_000_000, "uatom")],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("frozen"));
    }

    // ========================================================================
    // Test: Deposit - With Affiliate
    // ========================================================================

    #[test]
    fn test_deposit_with_affiliate() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        let affiliate_addr = app.api().addr_make("affiliate");

        // Deposit with affiliate
        let res = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::Deposit {
                    position_id: None,
                    position_owner: None,
                    affiliate_address: Some(affiliate_addr.to_string()),
                },
                &[coin(1_000_000, "uatom")],
            )
            .unwrap();

        // Deposit should succeed -- affiliate is passed to Debt.NotifyDeposit SubMsg
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "deposit")
        }));
    }

    // ========================================================================
    // Test: Authorization - Only Debt Contract Can Call AuthorizedWithdraw
    // ========================================================================

    #[test]
    fn test_authorized_withdraw_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position first
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Try AuthorizedWithdraw from user (not the debt contract)
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::AuthorizedWithdraw {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    assets: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(500_000),
                    }],
                    send_to: user.to_string(),
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: Authorization - Only Debt Contract Can Call ActivateSupplyCaps
    // ========================================================================

    #[test]
    fn test_activate_supply_caps_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Try ActivateSupplyCaps from user (not debt contract)
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::ActivateSupplyCaps {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: Authorization - Only Debt Contract Can Call DeactivateSupplyCaps
    // ========================================================================

    #[test]
    fn test_deactivate_supply_caps_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Try DeactivateSupplyCaps from user (not debt contract)
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::DeactivateSupplyCaps {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: Authorization - Debt Contract CAN Call ActivateSupplyCaps
    // ========================================================================

    #[test]
    fn test_activate_supply_caps_authorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position with uatom
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // ActivateSupplyCaps from debt contract should succeed
        let res = app
            .execute_contract(
                contracts.debt_addr.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::ActivateSupplyCaps {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                },
                &[],
            )
            .unwrap();
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "activate_supply_caps")
        }));

        // Check that supply caps were updated
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        // uatom supply cap should have current_supply = 1_000_000
        let atom_cap = basket
            .collateral_supply_caps
            .iter()
            .find(|cap| cap.asset_info.to_string() == "uatom")
            .unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::new(1_000_000));
    }

    // ========================================================================
    // Test: Authorization - Debt Contract CAN Call DeactivateSupplyCaps
    // ========================================================================

    #[test]
    fn test_deactivate_supply_caps_authorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position and activate caps
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        app.execute_contract(
            contracts.debt_addr.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::ActivateSupplyCaps {
                position_id: Uint128::new(1),
                position_owner: user.to_string(),
            },
            &[],
        )
        .unwrap();

        // Deactivate caps
        app.execute_contract(
            contracts.debt_addr.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::DeactivateSupplyCaps {
                position_id: Uint128::new(1),
                position_owner: user.to_string(),
            },
            &[],
        )
        .unwrap();

        // Check that supply caps were cleared
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        let atom_cap = basket
            .collateral_supply_caps
            .iter()
            .find(|cap| cap.asset_info.to_string() == "uatom")
            .unwrap();
        assert_eq!(atom_cap.current_supply, Uint128::zero());
    }

    // ========================================================================
    // Test: Authorization - Only Liquidation Contract Can Call LiquidationClaimUpdate
    // ========================================================================

    #[test]
    fn test_liquidation_claim_update_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Try LiquidationClaimUpdate from user (not liquidation contract)
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::LiquidationClaimUpdate {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    collateral_reductions: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(500_000),
                    }],
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: Authorization - Liquidation Contract CAN Call LiquidationClaimUpdate
    // ========================================================================

    #[test]
    fn test_liquidation_claim_update_authorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // LiquidationClaimUpdate from liquidation contract
        let res = app
            .execute_contract(
                contracts.liq_addr.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::LiquidationClaimUpdate {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    collateral_reductions: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(500_000),
                    }],
                },
                &[],
            )
            .unwrap();
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "liquidation_claim_update")
        }));

        // Verify position was updated (500k reduced from 1M)
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0].asset.amount,
            Uint128::new(500_000)
        );
    }

    // ========================================================================
    // Test: Authorization - Only Liquidation Contract Can Call AuthorizedTransfer
    // ========================================================================

    #[test]
    fn test_authorized_transfer_unauthorized() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Try AuthorizedTransfer from user (not liquidation contract)
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::AuthorizedTransfer {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    assets: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(500_000),
                    }],
                    recipient: user.to_string(),
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: EditBasket - Admin Only
    // ========================================================================

    #[test]
    fn test_edit_basket_admin_only() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let admin = get_admin_addr(&app);

        // User (non-admin) tries to edit basket
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::EditBasket(EditBasket {
                    added_cAsset: Some(make_casset("uusdc", 60, 70)),
                    collateral_supply_caps: None,
                    multi_asset_supply_caps: None,
                }),
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));

        // Admin can edit basket
        let res = app
            .execute_contract(
                admin.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::EditBasket(EditBasket {
                    added_cAsset: Some(make_casset("uusdc", 60, 70)),
                    collateral_supply_caps: None,
                    multi_asset_supply_caps: None,
                }),
                &[],
            )
            .unwrap();
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "edit_basket")
        }));

        // Verify basket was updated
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        assert_eq!(basket.collateral_types.len(), 3);
    }

    // ========================================================================
    // Test: EditcAsset - Admin Only
    // ========================================================================

    #[test]
    fn test_edit_casset_admin_only() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let admin = get_admin_addr(&app);

        // User tries to edit cAsset
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::EditcAsset {
                    asset: AssetInfo::NativeToken {
                        denom: "uatom".to_string(),
                    },
                    max_borrow_LTV: Some(Decimal::percent(55)),
                    max_LTV: Some(Decimal::percent(65)),
                    max_threshold_to_delay: None,
                },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));

        // Admin can edit cAsset
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "uatom".to_string(),
                },
                max_borrow_LTV: Some(Decimal::percent(55)),
                max_LTV: Some(Decimal::percent(65)),
                max_threshold_to_delay: None,
            },
            &[],
        )
        .unwrap();

        // Verify basket was updated
        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        let atom_casset = basket
            .collateral_types
            .iter()
            .find(|ct| ct.asset.info.to_string() == "uatom")
            .unwrap();
        assert_eq!(atom_casset.max_borrow_LTV, Decimal::percent(55));
        assert_eq!(atom_casset.max_LTV, Decimal::percent(65));
    }

    // ========================================================================
    // Test: SetFrozen - Admin Only
    // ========================================================================

    #[test]
    fn test_set_frozen_admin_only() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let admin = get_admin_addr(&app);

        // User tries to freeze
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::SetFrozen { frozen: true },
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));

        // Admin can freeze
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: true },
            &[],
        )
        .unwrap();

        // Verify frozen
        let frozen: bool = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetFrozen {},
            )
            .unwrap();
        assert!(frozen);

        // Admin can unfreeze
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::SetFrozen { frozen: false },
            &[],
        )
        .unwrap();
        let frozen: bool = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetFrozen {},
            )
            .unwrap();
        assert!(!frozen);
    }

    // ========================================================================
    // Test: UpdateConfig - Admin Only
    // ========================================================================

    #[test]
    fn test_update_config_admin_only() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let admin = get_admin_addr(&app);

        // User tries to update config
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::UpdateConfig(UpdateConfig {
                    owner: None,
                    oracle_contract: None,
                    chain_proxy: None,
                    ltv_disco: None,
                    debt_contract: None,
                    liquidation_contract: None,
                    collateral_twap_timeframe: Some(120u64),
                    oracle_time_limit: None,
                    ltv_upward_kp: None,
                    ltv_downward_period: None,
                    ltv_downward_decay_rate: None,
                    ltv_borrow_distance: None,
                }),
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));

        // Admin updates config
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                oracle_contract: None,
                chain_proxy: None,
                ltv_disco: None,
                debt_contract: None,
                liquidation_contract: None,
                collateral_twap_timeframe: Some(120u64),
                oracle_time_limit: Some(600u64),
                ltv_upward_kp: None,
                ltv_downward_period: None,
                ltv_downward_decay_rate: None,
                ltv_borrow_distance: None,
            }),
            &[],
        )
        .unwrap();

        // Verify config was updated
        let config: Config = app
            .wrap()
            .query_wasm_smart(contracts.collateral_addr.clone(), &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.collateral_twap_timeframe, 120u64);
        assert_eq!(config.oracle_time_limit, 600u64);
    }

    // ========================================================================
    // Test: AuthorizedWithdraw from Debt Contract - Success
    // ========================================================================

    #[test]
    fn test_authorized_withdraw_success() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Debt contract calls AuthorizedWithdraw
        let res = app
            .execute_contract(
                contracts.debt_addr.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::AuthorizedWithdraw {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    assets: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(500_000),
                    }],
                    send_to: user.to_string(),
                },
                &[],
            )
            .unwrap();
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "authorized_withdraw")
        }));

        // Verify position was updated (500k remaining)
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0].asset.amount,
            Uint128::new(500_000)
        );
    }

    // ========================================================================
    // Test: AuthorizedWithdraw - Full Withdrawal Removes cAsset
    // ========================================================================

    #[test]
    fn test_authorized_withdraw_full_removes_casset() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position with uatom and uosmo
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")],
        )
        .unwrap();

        // Fully withdraw uatom via AuthorizedWithdraw
        app.execute_contract(
            contracts.debt_addr.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::AuthorizedWithdraw {
                position_id: Uint128::new(1),
                position_owner: user.to_string(),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uatom".to_string(),
                    },
                    amount: Uint128::new(1_000_000),
                }],
                send_to: user.to_string(),
            },
            &[],
        )
        .unwrap();

        // Position should now only have uosmo
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            positions_resp.positions[0].collateral_assets.len(),
            1
        );
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0]
                .asset
                .info
                .to_string(),
            "uosmo"
        );
    }

    // ========================================================================
    // Test: AuthorizedTransfer - Success
    // ========================================================================

    #[test]
    fn test_authorized_transfer_success() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let recipient = app.api().addr_make("liq_queue");

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Liquidation contract transfers collateral
        let res = app
            .execute_contract(
                contracts.liq_addr.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::AuthorizedTransfer {
                    position_id: Uint128::new(1),
                    position_owner: user.to_string(),
                    assets: vec![Asset {
                        info: AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        amount: Uint128::new(300_000),
                    }],
                    recipient: recipient.to_string(),
                },
                &[],
            )
            .unwrap();
        assert!(res.events.iter().any(|e| {
            e.attributes
                .iter()
                .any(|a| a.key == "method" && a.value == "authorized_transfer")
        }));

        // Verify position was updated (700k remaining)
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0].asset.amount,
            Uint128::new(700_000)
        );
    }

    // ========================================================================
    // Test: Query - GetCollateralPositions by UserInfo
    // ========================================================================

    #[test]
    fn test_query_positions_by_user_info() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Query by user_info
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: Some(UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: user.to_string(),
                    }),
                    user: None,
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 1);
        assert_eq!(positions_resp.positions[0].position_id, Uint128::new(1));
    }

    // ========================================================================
    // Test: Query - Non-existent Position Returns Empty
    // ========================================================================

    #[test]
    fn test_query_nonexistent_position() {
        let (app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Query a user with no positions
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 0);
    }

    // ========================================================================
    // Test: EditBasket - Update Supply Caps
    // ========================================================================

    #[test]
    fn test_edit_basket_update_supply_caps() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        // Update uatom supply cap ratio
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "uatom".to_string(),
                    },
                    current_supply: Uint128::zero(), // ignored in edit
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(50), // was 100%
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                multi_asset_supply_caps: None,
            }),
            &[],
        )
        .unwrap();

        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        let atom_cap = basket
            .collateral_supply_caps
            .iter()
            .find(|cap| cap.asset_info.to_string() == "uatom")
            .unwrap();
        assert_eq!(atom_cap.supply_cap_ratio, Decimal::percent(50));
    }

    // ========================================================================
    // Test: Multiple Positions Per User
    // ========================================================================

    #[test]
    fn test_multiple_positions_per_user() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create first position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Create second position
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(2_000_000, "uosmo")],
        )
        .unwrap();

        // Query all user positions
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 2);
        assert_eq!(positions_resp.positions[0].position_id, Uint128::new(1));
        assert_eq!(positions_resp.positions[1].position_id, Uint128::new(2));
    }

    // ========================================================================
    // Test: Deposit - Position owner different from sender
    // ========================================================================

    #[test]
    fn test_deposit_on_behalf_of_another_user() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);
        let other_user = app.api().addr_make("other_user");

        // User deposits on behalf of other_user
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: Some(other_user.to_string()),
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom")],
        )
        .unwrap();

        // Position should be owned by other_user
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(other_user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(positions_resp.positions.len(), 1);

        // Original user should have no positions
        let user_positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(user_positions_resp.positions.len(), 0);
    }

    // ========================================================================
    // Test: Full Liquidation Claim Update Removes cAsset Entry
    // ========================================================================

    #[test]
    fn test_liquidation_claim_update_full_removal() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Create a position with uatom
        app.execute_contract(
            user.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::Deposit {
                position_id: None,
                position_owner: None,
                affiliate_address: None,
            },
            &[coin(1_000_000, "uatom"), coin(2_000_000, "uosmo")],
        )
        .unwrap();

        // Liquidation contract reduces all uatom
        app.execute_contract(
            contracts.liq_addr.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::LiquidationClaimUpdate {
                position_id: Uint128::new(1),
                position_owner: user.to_string(),
                collateral_reductions: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uatom".to_string(),
                    },
                    amount: Uint128::new(1_000_000),
                }],
            },
            &[],
        )
        .unwrap();

        // Position should only have uosmo now
        let positions_resp: CollateralPositionsResponse = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetCollateralPositions {
                    start_after: None,
                    limit: None,
                    user_info: None,
                    user: Some(user.to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            positions_resp.positions[0].collateral_assets.len(),
            1
        );
        assert_eq!(
            positions_resp.positions[0].collateral_assets[0]
                .asset
                .info
                .to_string(),
            "uosmo"
        );
    }

    // ========================================================================
    // Test: Query Config
    // ========================================================================

    #[test]
    fn test_query_config() {
        let (app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        let config: Config = app
            .wrap()
            .query_wasm_smart(contracts.collateral_addr.clone(), &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.owner, admin);
    }

    // ========================================================================
    // Test: Query GetBasket
    // ========================================================================

    #[test]
    fn test_query_basket() {
        let (app, contracts) = proper_instantiate();

        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        assert_eq!(basket.basket_id, Uint128::one());
        assert_eq!(basket.collateral_types.len(), 2);

        // Check uatom LTVs
        let atom_casset = &basket.collateral_types[0];
        assert_eq!(atom_casset.max_borrow_LTV, Decimal::percent(50));
        assert_eq!(atom_casset.max_LTV, Decimal::percent(60));

        // Check uosmo LTVs
        let osmo_casset = &basket.collateral_types[1];
        assert_eq!(osmo_casset.max_borrow_LTV, Decimal::percent(40));
        assert_eq!(osmo_casset.max_LTV, Decimal::percent(50));
    }

    // ========================================================================
    // Test: EditBasket - Add Multi-Asset Supply Cap
    // ========================================================================

    #[test]
    fn test_edit_basket_multi_asset_supply_cap() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);

        // Add a multi-asset supply cap
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: Some(vec![MultiAssetSupplyCap {
                    assets: vec![
                        AssetInfo::NativeToken {
                            denom: "uatom".to_string(),
                        },
                        AssetInfo::NativeToken {
                            denom: "uosmo".to_string(),
                        },
                    ],
                    supply_cap_ratio: Decimal::percent(80),
                }]),
            }),
            &[],
        )
        .unwrap();

        let basket: BasketCollateral = app
            .wrap()
            .query_wasm_smart(
                contracts.collateral_addr.clone(),
                &QueryMsg::GetBasket {},
            )
            .unwrap();
        assert_eq!(basket.multi_asset_supply_caps.len(), 1);
        assert_eq!(
            basket.multi_asset_supply_caps[0].supply_cap_ratio,
            Decimal::percent(80)
        );
    }

    // ========================================================================
    // Test: UpdateConfig - Transfer Ownership
    // ========================================================================

    #[test]
    fn test_update_config_transfer_ownership() {
        let (mut app, contracts) = proper_instantiate();
        let admin = get_admin_addr(&app);
        let new_owner = app.api().addr_make("new_owner");

        // Transfer ownership
        app.execute_contract(
            admin.clone(),
            contracts.collateral_addr.clone(),
            &ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: Some(new_owner.to_string()),
                oracle_contract: None,
                chain_proxy: None,
                ltv_disco: None,
                debt_contract: None,
                liquidation_contract: None,
                collateral_twap_timeframe: None,
                oracle_time_limit: None,
                ltv_upward_kp: None,
                ltv_downward_period: None,
                ltv_downward_decay_rate: None,
                ltv_borrow_distance: None,
            }),
            &[],
        )
        .unwrap();

        // Verify new owner
        let config: Config = app
            .wrap()
            .query_wasm_smart(contracts.collateral_addr.clone(), &QueryMsg::Config {})
            .unwrap();
        assert_eq!(config.owner, new_owner);

        // Old admin can no longer update
        let err = app
            .execute_contract(
                admin.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::UpdateConfig(UpdateConfig {
                    owner: None,
                    oracle_contract: None,
                    chain_proxy: None,
                    ltv_disco: None,
                    debt_contract: None,
                    liquidation_contract: None,
                    collateral_twap_timeframe: Some(300u64),
                    oracle_time_limit: None,
                    ltv_upward_kp: None,
                    ltv_downward_period: None,
                    ltv_downward_decay_rate: None,
                    ltv_borrow_distance: None,
                }),
                &[],
            )
            .unwrap_err();
        assert!(err.root_cause().to_string().contains("Unauthorized"));
    }

    // ========================================================================
    // Test: Deposit Non-Existent Position Errors
    // ========================================================================

    #[test]
    fn test_deposit_nonexistent_position_id() {
        let (mut app, contracts) = proper_instantiate();
        let user = get_user_addr(&app);

        // Try to deposit into position_id=99 which doesn't exist
        let err = app
            .execute_contract(
                user.clone(),
                contracts.collateral_addr.clone(),
                &ExecuteMsg::Deposit {
                    position_id: Some(Uint128::new(99)),
                    position_owner: None,
                    affiliate_address: None,
                },
                &[coin(1_000_000, "uatom")],
            )
            .unwrap_err();
        // Should error because position doesn't exist
        assert!(
            err.root_cause().to_string().contains("No user positions")
                || err.root_cause().to_string().contains("does not exist")
        );
    }
}
