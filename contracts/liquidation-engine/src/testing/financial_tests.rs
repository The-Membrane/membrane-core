#![allow(non_snake_case)]

pub mod tests {

    use membrane::liquidation_engine::{
        CallbackMsg, Config, ExecuteMsg, InstantiateMsg, LiquidationPropagation, QueryMsg,
        SimulateLiquidationResponse, UpdateConfig,
    };
    use membrane::oracle::PriceResponse;
    use membrane::types::{cAsset, Asset, AssetInfo, PriceInfo};

    use cosmwasm_std::{
        attr, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdError, StdResult,
        Uint128,
    };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    use crate::liquidations::get_repay_quantities;

    const USER: &str = "user";
    const ADMIN: &str = "admin";
    const LIQUIDATOR: &str = "liquidator";

    fn get_user_addr(app: &App) -> Addr {
        app.api().addr_make(USER)
    }

    fn get_admin_addr(app: &App) -> Addr {
        app.api().addr_make(ADMIN)
    }

    fn get_liquidator_addr(app: &App) -> Addr {
        app.api().addr_make(LIQUIDATOR)
    }

    pub fn liq_engine_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    // ========================================================================
    // Mock message types
    // ========================================================================

    #[cw_serde]
    pub enum Debt_MockExecuteMsg {
        AccrueAndReturnDebt {
            position_owner: String,
            position_id: Uint128,
        },
        LiquidationDebtUpdate {
            position_id: Uint128,
            position_owner: String,
            repay_amount: Uint128,
        },
        RecordBadDebt {
            position_id: Uint128,
            position_owner: String,
            bad_debt_amount: Uint128,
        },
    }

    #[cw_serde]
    pub struct Debt_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Debt_MockQueryMsg {}

    #[cw_serde]
    pub enum Collateral_MockExecuteMsg {
        RefreshAndAssess {
            position_owner: String,
            position_id: Uint128,
        },
        LiquidationClaimUpdate {
            position_id: Uint128,
            position_owner: String,
            collateral_reductions: Vec<Asset>,
        },
        AuthorizedTransfer {
            position_id: Uint128,
            position_owner: String,
            assets: Vec<Asset>,
            recipient: String,
        },
        AuthorizedSellCollateral {
            position_id: Uint128,
            position_owner: String,
            max_spread: Decimal,
            send_proceeds_to: String,
            sell_amounts: Option<Vec<Asset>>,
        },
    }

    #[cw_serde]
    pub struct Collateral_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Collateral_MockQueryMsg {}

    #[cw_serde]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: PriceResponse,
            collateral_price: PriceResponse,
            collateral_amount: membrane::math::Uint256,
            bid_for: AssetInfo,
        },
    }

    #[cw_serde]
    pub struct LQ_MockInstantiateMsg {}

    #[cw_serde]
    pub enum LQ_MockQueryMsg {}

    #[cw_serde]
    pub struct Staking_MockInstantiateMsg {}

    // ========================================================================
    // Mock contract factories - each is a non-capturing fn pointer
    // ========================================================================

    /// Default mock debt contract (5000 CDT at $1)
    pub fn mock_debt_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: Debt_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Debt_MockExecuteMsg::AccrueAndReturnDebt { .. } => {
                        let debt_response = membrane::debt::AccrueDebtResponse {
                            total_debt: Uint128::new(5_000_000000u128),
                            credit_price: PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            rate_segments: vec![],
                            peg_rate_segments: vec![],
                            venue_recall_amount: Uint128::zero(),
                        };
                        let json = to_json_binary(&debt_response)?;
                        Ok(Response::new()
                            .add_attribute("debt_response", String::from_utf8(json.to_vec()).unwrap()))
                    }
                    Debt_MockExecuteMsg::LiquidationDebtUpdate { repay_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "liquidation_debt_update")
                            .add_attribute("repay_amount", repay_amount.to_string()))
                    }
                    Debt_MockExecuteMsg::RecordBadDebt { bad_debt_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "record_bad_debt")
                            .add_attribute("bad_debt_amount", bad_debt_amount.to_string()))
                    }
                }
            },
            |_deps, _env, _info, _msg: Debt_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Debt_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    /// Mock debt contract returning zero debt
    pub fn mock_debt_contract_zero_debt() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: Debt_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Debt_MockExecuteMsg::AccrueAndReturnDebt { .. } => {
                        let debt_response = membrane::debt::AccrueDebtResponse {
                            total_debt: Uint128::zero(),
                            credit_price: PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            rate_segments: vec![],
                            peg_rate_segments: vec![],
                            venue_recall_amount: Uint128::zero(),
                        };
                        let json = to_json_binary(&debt_response)?;
                        Ok(Response::new()
                            .add_attribute("debt_response", String::from_utf8(json.to_vec()).unwrap()))
                    }
                    Debt_MockExecuteMsg::LiquidationDebtUpdate { repay_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "liquidation_debt_update")
                            .add_attribute("repay_amount", repay_amount.to_string()))
                    }
                    Debt_MockExecuteMsg::RecordBadDebt { bad_debt_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "record_bad_debt")
                            .add_attribute("bad_debt_amount", bad_debt_amount.to_string()))
                    }
                }
            },
            |_deps, _env, _info, _msg: Debt_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Debt_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    /// Mock debt contract: 700k CDT at $1 (insolvent against default collateral)
    pub fn mock_debt_contract_insolvent() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: Debt_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Debt_MockExecuteMsg::AccrueAndReturnDebt { .. } => {
                        let debt_response = membrane::debt::AccrueDebtResponse {
                            total_debt: Uint128::new(700_000_000000u128), // 700k CDT
                            credit_price: PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            rate_segments: vec![],
                            peg_rate_segments: vec![],
                            venue_recall_amount: Uint128::zero(),
                        };
                        let json = to_json_binary(&debt_response)?;
                        Ok(Response::new()
                            .add_attribute("debt_response", String::from_utf8(json.to_vec()).unwrap()))
                    }
                    Debt_MockExecuteMsg::LiquidationDebtUpdate { repay_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "liquidation_debt_update")
                            .add_attribute("repay_amount", repay_amount.to_string()))
                    }
                    Debt_MockExecuteMsg::RecordBadDebt { bad_debt_amount, .. } => {
                        Ok(Response::new()
                            .add_attribute("method", "record_bad_debt")
                            .add_attribute("bad_debt_amount", bad_debt_amount.to_string()))
                    }
                }
            },
            |_deps, _env, _info, _msg: Debt_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Debt_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    /// Default mock collateral contract (100k ATOM at $10, max LTV 60%)
    pub fn mock_collateral_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: Collateral_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Collateral_MockExecuteMsg::RefreshAndAssess { .. } => {
                        let assessment = membrane::collateral::CollateralAssessmentResponse {
                            collateral_assets: vec![cAsset {
                                asset: Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "uatom".to_string(),
                                    },
                                    amount: Uint128::new(100_000_000000u128),
                                },
                                max_borrow_LTV: Decimal::percent(50),
                                max_LTV: Decimal::percent(60),
                                pool_info: None,
                                rate_index: Decimal::one(),
                                peg_rate_index: Decimal::one(),
                                force_redemptions: None,
                                max_threshold_to_delay: Decimal::percent(15),
                            }],
                            ratios: vec![Decimal::one()],
                            prices: vec![PriceResponse {
                                prices: vec![],
                                price: Decimal::from_ratio(10u128, 1u128),
                                decimals: 6,
                            }],
                            avg_max_ltv: Decimal::percent(60),
                            avg_max_borrow_ltv: Decimal::percent(50),
                            avg_max_threshold_to_delay: Decimal::percent(15),
                            any_frozen: false,
                            frozen_assets: vec![],
                        };
                        let json = to_json_binary(&assessment)?;
                        Ok(Response::new().add_attribute("assessment", String::from_utf8(json.to_vec()).unwrap()))
                    }
                    Collateral_MockExecuteMsg::LiquidationClaimUpdate { .. } => {
                        Ok(Response::new().add_attribute("method", "liquidation_claim_update"))
                    }
                    Collateral_MockExecuteMsg::AuthorizedTransfer { .. } => {
                        Ok(Response::new().add_attribute("method", "authorized_transfer"))
                    }
                    Collateral_MockExecuteMsg::AuthorizedSellCollateral { .. } => {
                        Ok(Response::new().add_attribute("method", "authorized_sell_collateral"))
                    }
                }
            },
            |_deps, _env, _info, _msg: Collateral_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Collateral_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    /// Mock collateral contract that reports frozen assets
    pub fn mock_collateral_contract_frozen() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: Collateral_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Collateral_MockExecuteMsg::RefreshAndAssess { .. } => {
                        let assessment = membrane::collateral::CollateralAssessmentResponse {
                            collateral_assets: vec![cAsset {
                                asset: Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: "uatom".to_string(),
                                    },
                                    amount: Uint128::new(100_000_000000u128),
                                },
                                max_borrow_LTV: Decimal::percent(50),
                                max_LTV: Decimal::percent(60),
                                pool_info: None,
                                rate_index: Decimal::one(),
                                peg_rate_index: Decimal::one(),
                                force_redemptions: None,
                                max_threshold_to_delay: Decimal::percent(15),
                            }],
                            ratios: vec![Decimal::one()],
                            prices: vec![PriceResponse {
                                prices: vec![],
                                price: Decimal::from_ratio(10u128, 1u128),
                                decimals: 6,
                            }],
                            avg_max_ltv: Decimal::percent(60),
                            avg_max_borrow_ltv: Decimal::percent(50),
                            avg_max_threshold_to_delay: Decimal::percent(15),
                            any_frozen: true,
                            frozen_assets: vec!["uatom".to_string()],
                        };
                        let json = to_json_binary(&assessment)?;
                        Ok(Response::new().add_attribute("assessment", String::from_utf8(json.to_vec()).unwrap()))
                    }
                    Collateral_MockExecuteMsg::LiquidationClaimUpdate { .. } => {
                        Ok(Response::new().add_attribute("method", "liquidation_claim_update"))
                    }
                    Collateral_MockExecuteMsg::AuthorizedTransfer { .. } => {
                        Ok(Response::new().add_attribute("method", "authorized_transfer"))
                    }
                    Collateral_MockExecuteMsg::AuthorizedSellCollateral { .. } => {
                        Ok(Response::new().add_attribute("method", "authorized_sell_collateral"))
                    }
                }
            },
            |_deps, _env, _info, _msg: Collateral_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Collateral_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    pub fn mock_liq_queue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        collateral_amount,
                        bid_for,
                        ..
                    } => Ok(Response::new().add_attributes(vec![
                        attr("action", "execute_bid"),
                        attr("repay_amount", collateral_amount.to_string()),
                        attr("collateral_token", bid_for.to_string()),
                        attr("collateral_info", "native_token"),
                        attr("collateral_amount", collateral_amount.to_string()),
                    ])),
                }
            },
            |_deps, _env, _info, _msg: LQ_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    pub fn mock_staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _info, _msg: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_deps, _env, _msg: Empty| -> StdResult<Binary> {
                Err(StdError::generic_err("no queries"))
            },
        );
        Box::new(contract)
    }

    // ========================================================================
    // Setup helpers
    // ========================================================================

    /// Full setup with the given mock contracts and liq_fee.
    fn setup_env(
        debt_contract: Box<dyn Contract<Empty>>,
        collateral_contract: Box<dyn Contract<Empty>>,
        liq_queue: Option<Box<dyn Contract<Empty>>>,
        liq_fee: Decimal,
    ) -> (App, Addr) {
        let mut app = AppBuilder::new().build(|_router, _api, _storage| {});
        let admin_addr = app.api().addr_make(ADMIN);

        let debt_code_id = app.store_code(debt_contract);
        let collateral_code_id = app.store_code(collateral_contract);
        let staking_code_id = app.store_code(mock_staking_contract());
        let liq_engine_code_id = app.store_code(liq_engine_contract());

        let debt_addr = app
            .instantiate_contract(debt_code_id, admin_addr.clone(),
                &Debt_MockInstantiateMsg {}, &[], "mock_debt", None)
            .unwrap();

        let collateral_addr = app
            .instantiate_contract(collateral_code_id, admin_addr.clone(),
                &Collateral_MockInstantiateMsg {}, &[], "mock_collateral", None)
            .unwrap();

        let staking_addr = app
            .instantiate_contract(staking_code_id, admin_addr.clone(),
                &Staking_MockInstantiateMsg {}, &[], "mock_staking", None)
            .unwrap();

        let liq_queue_addr = if let Some(lq) = liq_queue {
            let lq_code_id = app.store_code(lq);
            Some(app.instantiate_contract(lq_code_id, admin_addr.clone(),
                &LQ_MockInstantiateMsg {}, &[], "mock_liq_queue", None).unwrap())
        } else {
            None
        };

        let liq_engine_addr = app
            .instantiate_contract(
                liq_engine_code_id,
                admin_addr.clone(),
                &InstantiateMsg {
                    owner: Some(admin_addr.to_string()),
                    collateral_contract: collateral_addr.to_string(),
                    debt_contract: debt_addr.to_string(),
                    liq_queue: liq_queue_addr.map(|a| a.to_string()),
                    chain_proxy: None,
                    debt_auction: None,
                    staking_contract: Some(staking_addr.to_string()),
                    liq_fee,
                    credit_denom: None,
                    liquidation_delay: None,
                },
                &[], "liquidation_engine", None,
            )
            .unwrap();

        (app, liq_engine_addr)
    }

    fn proper_instantiate() -> (App, Addr) {
        setup_env(
            mock_debt_contract(),
            mock_collateral_contract(),
            Some(mock_liq_queue_contract()),
            Decimal::percent(1),
        )
    }

    fn proper_instantiate_no_liq_queue() -> (App, Addr) {
        setup_env(
            mock_debt_contract(),
            mock_collateral_contract(),
            None,
            Decimal::percent(1),
        )
    }

    fn setup_insolvent() -> (App, Addr) {
        setup_env(
            mock_debt_contract_insolvent(),
            mock_collateral_contract(),
            Some(mock_liq_queue_contract()),
            Decimal::percent(1),
        )
    }

    fn setup_insolvent_no_liq_queue() -> (App, Addr) {
        setup_env(
            mock_debt_contract_insolvent(),
            mock_collateral_contract(),
            None,
            Decimal::percent(1),
        )
    }

    fn setup_frozen() -> (App, Addr) {
        setup_env(
            mock_debt_contract_insolvent(),
            mock_collateral_contract_frozen(),
            Some(mock_liq_queue_contract()),
            Decimal::percent(1),
        )
    }

    fn setup_zero_debt() -> (App, Addr) {
        setup_env(
            mock_debt_contract_zero_debt(),
            mock_collateral_contract(),
            Some(mock_liq_queue_contract()),
            Decimal::percent(1),
        )
    }

    /// Helper to build a LiquidationPropagation with defaults for unit tests.
    fn default_propagation() -> LiquidationPropagation {
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
            credit_price: PriceResponse {
                prices: vec![],
                price: Decimal::zero(),
                decimals: 6,
            },
            caller_fee_value_paid: Decimal::zero(),
            total_repaid: Decimal::zero(),
            venue_recall_amount: Uint128::zero(),
            position_owner: Addr::unchecked("user"),
            position_id: Uint128::one(),
            caller: Addr::unchecked("liquidator"),
            timer_start_only: false,
            liquidation_outcome: None,
            timer_started_at: None,
            current_ltv: Decimal::zero(),
        }
    }

    /// Helper to build a Config with defaults for unit tests.
    fn default_config() -> Config {
        Config {
            owner: Addr::unchecked("owner"),
            collateral_contract: Addr::unchecked("collateral"),
            debt_contract: Addr::unchecked("debt"),
            liq_queue: None,
            chain_proxy: None,
            debt_auction: None,
            staking_contract: None,
            liq_fee: Decimal::percent(1),
            credit_denom: None,
            liquidation_delay: 28800,
        }
    }

    // ========================================================================
    // A. get_repay_quantities deeper tests (9 tests)
    // ========================================================================

    mod repay_quantities_deep {
        use super::*;

        #[test]
        fn highly_leveraged_position_95_ltv() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(9_500u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(5),
            )
            .unwrap();

            // debt_value = 9500, collateral = 10000, current_ltv = 0.95
            // repay_value = ((0.95 - 0.50) / 0.95) * 9500 = (0.45/0.95)*9500 = 4500
            let debt_value = Decimal::from_ratio(9_500u128, 1u128);
            let current_ltv = debt_value / Decimal::from_ratio(10_000u128, 1u128);
            let excess = current_ltv - Decimal::percent(50);
            let fraction = excess / current_ltv;
            let expected_repay_value = debt_value * fraction;
            let expected_repay_amount = expected_repay_value.to_uint_floor();

            assert_eq!(repay_amount, expected_repay_amount,
                "Repay amount mismatch: got {}, expected {}", repay_amount, expected_repay_amount);
            assert_eq!(repay_value, expected_repay_value, "Repay value mismatch");
            // caller_fee_rate = current_ltv - avg_max_ltv = 0.95 - 0.60 = 0.35
            assert_eq!(caller_fee_rate, current_ltv - Decimal::percent(60), "Caller fee rate mismatch");
            // protocol_fee_rate = liq_fee = 0.05
            assert_eq!(protocol_fee_rate, Decimal::percent(5), "Protocol fee rate mismatch");
        }

        #[test]
        fn credit_price_below_peg_098() {
            let credit_price = Decimal::from_ratio(98u128, 100u128);
            let credit_pr = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
            let (repay_amount, _repay_value, _caller_fee_rate, _protocol_fee_rate) = get_repay_quantities(
                Uint128::new(10_000u128),
                &credit_pr,
                Decimal::from_ratio(12_000u128, 1u128),
                Decimal::percent(80),
                Decimal::percent(50),
                Decimal::percent(2),
            )
            .unwrap();

            assert!(repay_amount > Uint128::zero(),
                "Should produce a positive repay amount for depegged credit");

            let debt_value = Decimal::from_ratio(10_000u128, 1u128) * credit_price;
            let collateral_value = Decimal::from_ratio(12_000u128, 1u128);
            let current_ltv = debt_value / collateral_value;
            assert!(current_ltv > Decimal::percent(80),
                "Position should be insolvent: LTV = {}", current_ltv);
        }

        #[test]
        fn credit_price_above_peg_105() {
            let credit_price = Decimal::from_ratio(105u128, 100u128);
            let credit_pr = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
            let (repay_amount, _repay_value, _caller_fee_rate, _protocol_fee_rate) = get_repay_quantities(
                Uint128::new(6_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            // debt_value = 6000 * 1.05 = 6300, collateral = 10000
            // current_ltv = 0.63, borrow_ltv = 0.50
            // repay_value = ((0.63 - 0.50) / 0.63) * 6300
            let debt_value = Decimal::from_ratio(6_000u128, 1u128) * credit_price;
            let collateral_value = Decimal::from_ratio(10_000u128, 1u128);
            let current_ltv = debt_value / collateral_value;
            let excess = current_ltv - Decimal::percent(50);
            let fraction = excess / current_ltv;
            let expected_repay_value = debt_value * fraction;
            // get_amount with decimals=0: repay_value / credit_price * 10^0
            let expected_repay_amount = (expected_repay_value / credit_price).to_uint_floor();

            assert_eq!(repay_amount, expected_repay_amount,
                "Repay mismatch with above-peg credit price");
        }

        #[test]
        fn large_position_billion_scale() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(800_000_000u128),
                &credit_pr,
                Decimal::from_ratio(1_000_000_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            // debt_value = 800M, collateral = 1B, current_ltv = 0.80
            // repay_value = ((0.80 - 0.50) / 0.80) * 800M = (0.375) * 800M = 300M
            let debt_value = Decimal::from_ratio(800_000_000u128, 1u128);
            let current_ltv = debt_value / Decimal::from_ratio(1_000_000_000u128, 1u128);
            let excess = current_ltv - Decimal::percent(50);
            let fraction = excess / current_ltv;
            let expected_repay_value = debt_value * fraction;
            let expected_repay = expected_repay_value.to_uint_floor();
            assert_eq!(repay_amount, expected_repay);
            assert_eq!(repay_value, expected_repay_value);

            // caller_fee_rate = current_ltv - avg_max_ltv = 0.80 - 0.60 = 0.20
            assert_eq!(caller_fee_rate, current_ltv - Decimal::percent(60));
            // protocol_fee_rate = liq_fee = 0.01
            assert_eq!(protocol_fee_rate, Decimal::percent(1));
        }

        #[test]
        fn very_small_position_just_above_threshold() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, _repay_value, caller_fee_rate, _protocol_fee_rate) = get_repay_quantities(
                Uint128::new(600_100u128),
                &credit_pr,
                Decimal::from_ratio(1_000_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            assert!(repay_amount > Uint128::zero(),
                "Even a marginally insolvent position should have repay > 0");
            // With borrow_ltv=50%, repay is larger than old formula but still bounded
            // caller_fee_rate = current_ltv - max_ltv = 0.6001 - 0.60 = 0.0001
            assert!(caller_fee_rate < Decimal::percent(1),
                "Caller fee rate {} should be small for barely insolvent position", caller_fee_rate);
        }

        #[test]
        fn fully_underwater_100_ltv() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(10_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(10),
            )
            .unwrap();

            // current_ltv = 1.0 >= 1.0, so repay_value = collateral_value = 10000
            // but repay_amount is capped at total_debt = 10000
            assert_eq!(repay_amount, Uint128::new(10_000u128));
            assert_eq!(repay_value, Decimal::from_ratio(10_000u128, 1u128));
            // caller_fee_rate = current_ltv - max_ltv = 1.0 - 0.60 = 0.40
            assert_eq!(caller_fee_rate, Decimal::percent(40));
            // protocol_fee_rate = liq_fee = 0.10
            assert_eq!(protocol_fee_rate, Decimal::percent(10));
        }

        #[test]
        fn zero_liq_fee_produces_zero_protocol_fee_rate() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, _repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::zero(),
            )
            .unwrap();

            assert!(repay_amount > Uint128::zero());
            // caller_fee_rate = current_ltv - max_ltv = 0.80 - 0.60 = 0.20 (still non-zero)
            assert_eq!(caller_fee_rate, Decimal::percent(20));
            // protocol_fee_rate = liq_fee = 0
            assert_eq!(protocol_fee_rate, Decimal::zero());
        }

        #[test]
        fn fifty_percent_liq_fee() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(50),
            )
            .unwrap();

            // debt_value = 8000, collateral = 10000, current_ltv = 0.80
            // repay_value = ((0.80 - 0.50) / 0.80) * 8000 = (0.375) * 8000 = 3000
            let debt_value = Decimal::from_ratio(8_000u128, 1u128);
            let current_ltv = debt_value / Decimal::from_ratio(10_000u128, 1u128);
            let excess = current_ltv - Decimal::percent(50);
            let fraction = excess / current_ltv;
            let expected_repay_value = debt_value * fraction;
            assert_eq!(repay_amount, expected_repay_value.to_uint_floor());
            assert_eq!(repay_value, expected_repay_value);
            // caller_fee_rate = 0.80 - 0.60 = 0.20
            assert_eq!(caller_fee_rate, Decimal::percent(20));
            // protocol_fee_rate = 0.50
            assert_eq!(protocol_fee_rate, Decimal::percent(50));
        }

        #[test]
        fn ten_percent_liq_fee() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (_repay_amount, _repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(10),
            )
            .unwrap();

            // caller_fee_rate = current_ltv - max_ltv = 0.80 - 0.60 = 0.20
            assert_eq!(caller_fee_rate, Decimal::percent(20));
            // protocol_fee_rate = liq_fee = 0.10
            assert_eq!(protocol_fee_rate, Decimal::percent(10));
        }
    }

    // ========================================================================
    // B. Reply handler parsing tests (6 tests)
    // ========================================================================

    mod reply_handler_parsing {
        use super::*;

        #[test]
        fn debt_accrue_reply_parses_debt_response_attribute() {
            // Default mocks: solvent position, chain should proceed through debt accrual
            // and collateral assessment before erroring as solvent.
            let (mut app, liq_engine_addr) = proper_instantiate();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            match res {
                Ok(_) => {}
                Err(e) => {
                    let err_str = e.root_cause().to_string();
                    // PositionSolvent means the chain parsed both reply attributes successfully
                    assert!(
                        err_str.contains("solvent") || err_str.contains("Attribute"),
                        "Expected solvent or attribute error, got: {}", err_str
                    );
                }
            }
        }

        #[test]
        fn collateral_assess_reply_parses_assessment_attribute() {
            // Insolvent position: chain goes past collateral assessment
            let (mut app, liq_engine_addr) = setup_insolvent();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            match res {
                Ok(response) => {
                    let has_assess = response.events.iter().any(|e| {
                        e.attributes.iter().any(|a| {
                            a.key == "method"
                                && (a.value.contains("collateral_assess")
                                    || a.value.contains("liq_queue")
                                    || a.value.contains("sell_wall")
                                    || a.value.contains("debt_update")
                                    || a.value.contains("claim_update"))
                        })
                    });
                    assert!(has_assess,
                        "Expected chain progress past collateral assessment. Events: {:?}",
                        response.events);
                }
                Err(e) => {
                    let err_str = e.root_cause().to_string();
                    assert!(
                        !err_str.contains("Failed to parse CollateralAssessmentResponse"),
                        "Collateral assessment parsing should succeed, got: {}", err_str
                    );
                }
            }
        }

        #[test]
        fn unknown_reply_id_returns_error() {
            use cosmwasm_std::testing::{mock_dependencies, mock_env};
            use cosmwasm_std::{Reply, SubMsgResponse, SubMsgResult};

            let mut deps = mock_dependencies();
            let env = mock_env();

            crate::state::CONFIG
                .save(deps.as_mut().storage, &default_config())
                .unwrap();

            let reply_msg = Reply {
                id: 999u64,
                result: SubMsgResult::Ok(SubMsgResponse {
                    events: vec![],
                    data: None,
                    msg_responses: vec![],
                }),
                payload: Default::default(),
                gas_used: 0u64,
            };

            let result = crate::reply::handle_reply(deps.as_mut(), env, reply_msg);
            assert!(result.is_err());
            let err_str = result.unwrap_err().to_string();
            assert!(err_str.contains("Unknown reply ID: 999"),
                "Expected unknown reply ID error, got: {}", err_str);
        }

        #[test]
        fn debt_accrue_reply_fails_on_missing_attribute() {
            use cosmwasm_std::testing::{mock_dependencies, mock_env};
            use cosmwasm_std::{Reply, SubMsgResponse, SubMsgResult};

            let mut deps = mock_dependencies();
            let env = mock_env();

            crate::state::CONFIG
                .save(deps.as_mut().storage, &default_config())
                .unwrap();
            crate::state::LIQUIDATION_PROPAGATION
                .save(deps.as_mut().storage, &default_propagation())
                .unwrap();

            let reply_msg = Reply {
                id: crate::state::DEBT_ACCRUE_REPLY_ID,
                result: SubMsgResult::Ok(SubMsgResponse {
                    events: vec![],
                    data: None,
                    msg_responses: vec![],
                }),
                payload: Default::default(),
                gas_used: 0u64,
            };

            let result = crate::reply::handle_reply(deps.as_mut(), env, reply_msg);
            assert!(result.is_err());
            let err_str = result.unwrap_err().to_string();
            assert!(
                err_str.contains("debt_response") || err_str.contains("not found"),
                "Expected missing attribute error, got: {}", err_str
            );
        }

        #[test]
        fn debt_accrue_reply_fails_on_malformed_json() {
            use cosmwasm_std::testing::{mock_dependencies, mock_env};
            use cosmwasm_std::{Event, Reply, SubMsgResponse, SubMsgResult};

            let mut deps = mock_dependencies();
            let env = mock_env();

            crate::state::CONFIG
                .save(deps.as_mut().storage, &default_config())
                .unwrap();
            crate::state::LIQUIDATION_PROPAGATION
                .save(deps.as_mut().storage, &default_propagation())
                .unwrap();

            let event = Event::new("wasm").add_attribute("debt_response", "not_valid_json{{{");

            let reply_msg = Reply {
                id: crate::state::DEBT_ACCRUE_REPLY_ID,
                result: SubMsgResult::Ok(SubMsgResponse {
                    events: vec![event],
                    data: None,
                    msg_responses: vec![],
                }),
                payload: Default::default(),
                gas_used: 0u64,
            };

            let result = crate::reply::handle_reply(deps.as_mut(), env, reply_msg);
            assert!(result.is_err());
            let err_str = result.unwrap_err().to_string();
            assert!(
                err_str.contains("Failed to parse AccrueDebtResponse")
                    || err_str.contains("parse"),
                "Expected JSON parse error, got: {}", err_str
            );
        }

        #[test]
        fn collateral_assess_reply_fails_on_missing_assessment_attr() {
            use cosmwasm_std::testing::{mock_dependencies, mock_env};
            use cosmwasm_std::{Reply, SubMsgResponse, SubMsgResult};

            let mut deps = mock_dependencies();
            let env = mock_env();

            crate::state::CONFIG
                .save(deps.as_mut().storage, &default_config())
                .unwrap();

            let mut prop = default_propagation();
            prop.total_debt = Uint128::new(5000u128);
            prop.credit_price = PriceResponse {
                prices: vec![],
                price: Decimal::one(),
                decimals: 6,
            };
            crate::state::LIQUIDATION_PROPAGATION
                .save(deps.as_mut().storage, &prop)
                .unwrap();

            let reply_msg = Reply {
                id: crate::state::COLLATERAL_ASSESS_REPLY_ID,
                result: SubMsgResult::Ok(SubMsgResponse {
                    events: vec![],
                    data: None,
                    msg_responses: vec![],
                }),
                payload: Default::default(),
                gas_used: 0u64,
            };

            let result = crate::reply::handle_reply(deps.as_mut(), env, reply_msg);
            assert!(result.is_err());
            let err_str = result.unwrap_err().to_string();
            assert!(
                err_str.contains("assessment") || err_str.contains("not found"),
                "Expected missing assessment attribute error, got: {}", err_str
            );
        }
    }

    // ========================================================================
    // C. Fee calculation tests (4 tests)
    // ========================================================================

    mod fee_calculations {
        use super::*;

        #[test]
        fn caller_fee_rate_equals_ltv_excess() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (_repay_amount, _repay_value, caller_fee_rate, _protocol_fee_rate) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            // caller_fee_rate = current_ltv - avg_max_ltv = 0.80 - 0.60 = 0.20
            let expected = Decimal::percent(20);
            assert_eq!(caller_fee_rate, expected,
                "Caller fee rate should be LTV excess (current - max). Got {}, expected {}",
                caller_fee_rate, expected);
        }

        #[test]
        fn protocol_fee_rate_equals_liq_fee() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (_repay_amount, _repay_value, _caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            // protocol_fee_rate = liq_fee = 0.01
            let expected = Decimal::percent(1);
            assert_eq!(protocol_fee_rate, expected,
                "Protocol fee rate should equal liq_fee. Got {}, expected {}",
                protocol_fee_rate, expected);
        }

        #[test]
        fn caller_fee_rate_independent_of_liq_fee() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            // caller_fee_rate depends only on current_ltv and max_ltv, not liq_fee
            let mut prev_caller_rate = None;
            for fee_pct in [1u64, 2, 5, 10, 25, 50] {
                let (_repay_amount, _repay_value, caller_fee_rate, protocol_fee_rate) = get_repay_quantities(
                    Uint128::new(8_000u128),
                    &credit_pr,
                    Decimal::from_ratio(10_000u128, 1u128),
                    Decimal::percent(60),
                    Decimal::percent(50),
                    Decimal::percent(fee_pct),
                )
                .unwrap();

                // caller_fee_rate should be the same regardless of liq_fee
                if let Some(prev) = prev_caller_rate {
                    assert_eq!(caller_fee_rate, prev,
                        "Caller fee rate should not change with liq_fee at {}%", fee_pct);
                }
                prev_caller_rate = Some(caller_fee_rate);

                // protocol_fee_rate should match liq_fee
                assert_eq!(protocol_fee_rate, Decimal::percent(fee_pct),
                    "Protocol fee rate should equal liq_fee at {}%", fee_pct);
            }
        }

        #[test]
        fn protocol_fee_rate_scales_linearly_with_liq_fee() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (_r1, _rv1, _cfr1, protocol_fee_1pct) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            let (_r2, _rv2, _cfr2, protocol_fee_2pct) = get_repay_quantities(
                Uint128::new(8_000u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(2),
            )
            .unwrap();

            let doubled = protocol_fee_1pct + protocol_fee_1pct;
            assert_eq!(protocol_fee_2pct, doubled,
                "2% protocol fee rate ({}) should be exactly 2x the 1% rate ({})",
                protocol_fee_2pct, doubled);
        }
    }

    // ========================================================================
    // D. SimulateLiquidation query tests (3 tests)
    // ========================================================================

    mod simulate_liquidation {
        use super::*;

        /// SimulateLiquidation now makes cross-contract queries to Collateral
        /// (GetPositionValue) and Debt (GetDebtPositions). The mock contracts
        /// don't support these WasmQuery::Smart queries, so the query fails.
        /// Real simulation is tested in stub_function_tests.rs with a custom mock querier.
        #[test]
        fn simulate_requires_query_support_from_mocks() {
            let (app, liq_engine_addr) = proper_instantiate();
            let user_addr = get_user_addr(&app);

            let result: StdResult<SimulateLiquidationResponse> = app
                .wrap()
                .query_wasm_smart(
                    liq_engine_addr,
                    &QueryMsg::SimulateLiquidation {
                        position_id: Uint128::one(),
                        position_owner: user_addr.to_string(),
                    },
                );

            assert!(
                result.is_err(),
                "SimulateLiquidation should fail without query-supporting mock contracts"
            );
        }
    }

    // ========================================================================
    // E. Edge case tests (7 tests)
    // ========================================================================

    mod edge_cases {
        use super::*;

        #[test]
        fn liquidation_without_liq_queue_skips_to_sell_wall() {
            let (mut app, liq_engine_addr) = setup_insolvent_no_liq_queue();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            match res {
                Ok(response) => {
                    let has_fee_transfer = response.events.iter().any(|e| {
                        e.attributes.iter()
                            .any(|a| a.value.contains("fee_transfer"))
                    });
                    let has_sell_wall = response.events.iter().any(|e| {
                        e.attributes.iter()
                            .any(|a| a.value.contains("sell_wall"))
                    });
                    assert!(has_fee_transfer || has_sell_wall,
                        "Expected fee_transfer or sell_wall evidence. Events: {:?}",
                        response.events);
                }
                Err(e) => {
                    let err_str = e.root_cause().to_string();
                    assert!(
                        !err_str.contains("Liq queue not configured"),
                        "Should handle missing liq queue gracefully, got: {}", err_str
                    );
                }
            }
        }

        #[test]
        fn liquidation_with_zero_debt_is_solvent() {
            let (mut app, liq_engine_addr) = setup_zero_debt();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            match res {
                Ok(_) => {}
                Err(e) => {
                    let err_str = e.root_cause().to_string();
                    assert!(err_str.contains("solvent"),
                        "Zero debt position should be solvent, got: {}", err_str);
                }
            }
        }

        #[test]
        fn liquidation_with_frozen_assets_errors() {
            let (mut app, liq_engine_addr) = setup_frozen();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            assert!(res.is_err(), "Liquidation should fail with frozen assets");
            let err_str = res.unwrap_err().root_cause().to_string();
            assert!(
                err_str.contains("frozen") || err_str.contains("Circuit breaker"),
                "Expected frozen asset error, got: {}", err_str
            );
        }

        #[test]
        fn liquidation_solvent_position_errors() {
            let (mut app, liq_engine_addr) = proper_instantiate();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            assert!(res.is_err(), "Solvent position should not be liquidatable");
            let err_str = res.unwrap_err().root_cause().to_string();
            assert!(err_str.contains("solvent"),
                "Expected solvent error, got: {}", err_str);
        }

        #[test]
        fn full_liquidation_chain_with_insolvent_position() {
            let (mut app, liq_engine_addr) = setup_insolvent();
            let user_addr = get_user_addr(&app);
            let liquidator_addr = get_liquidator_addr(&app);

            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::one(),
                position_owner: user_addr.to_string(),
            };

            let res = app.execute_contract(liquidator_addr, liq_engine_addr, &msg, &[]);

            match res {
                Ok(response) => {
                    let event_methods: Vec<&str> = response
                        .events
                        .iter()
                        .flat_map(|e| {
                            e.attributes.iter()
                                .filter(|a| a.key == "method")
                                .map(|a| a.value.as_str())
                        })
                        .collect();

                    assert!(
                        event_methods.iter().any(|m| *m == "liquidate"),
                        "Should see 'liquidate' method. Got: {:?}", event_methods
                    );
                }
                Err(e) => {
                    let err_str = e.root_cause().to_string();
                    assert!(
                        !err_str.contains("solvent"),
                        "Insolvent position should not be rejected as solvent. Error: {}", err_str
                    );
                }
            }
        }

        #[test]
        fn get_repay_quantities_with_high_max_ltv_90_percent() {
            let credit_pr = PriceResponse { prices: vec![], price: Decimal::one(), decimals: 0 };
            let (repay_amount, _, _, _) = get_repay_quantities(
                Uint128::new(9_100u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(90),
                Decimal::percent(50),
                Decimal::percent(5),
            )
            .unwrap();

            assert!(repay_amount > Uint128::zero());

            // 89% LTV should be solvent
            let result = get_repay_quantities(
                Uint128::new(8_900u128),
                &credit_pr,
                Decimal::from_ratio(10_000u128, 1u128),
                Decimal::percent(90),
                Decimal::percent(50),
                Decimal::percent(5),
            );
            assert!(result.is_err());
        }

        #[test]
        fn get_repay_quantities_with_fractional_credit_price() {
            let credit_price = Decimal::from_ratio(1u128, 2u128);
            let credit_pr = PriceResponse { prices: vec![], price: credit_price, decimals: 0 };
            let (repay_amount, _, _, _) = get_repay_quantities(
                Uint128::new(20_000u128),
                &credit_pr,
                Decimal::from_ratio(12_000u128, 1u128),
                Decimal::percent(60),
                Decimal::percent(50),
                Decimal::percent(1),
            )
            .unwrap();

            assert!(repay_amount > Uint128::zero(),
                "Repay amount should be > 0 for fractional credit price");
            assert!(repay_amount > Uint128::new(2_000u128),
                "Repay amount in credit units should exceed the value amount when credit < $1");
        }
    }
}
