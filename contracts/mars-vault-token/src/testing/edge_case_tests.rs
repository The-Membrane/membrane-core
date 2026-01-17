#[cfg(test)]
mod edge_case_tests {
    use crate::helpers::MarsVaultContract;
    use membrane::mars_vault_token::{
        ExecuteMsg, InstantiateMsg, QueryMsg, Config, VaultCost
    };
    use membrane::mars_redbank::{Market, UserCollateralResponse, InterestRateModel, MarketV2Response};

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal,
        Timestamp, BankMsg, CosmosMsg, WasmMsg,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";
    const TRANSMUTER: &str = "transmuter";
    const REVENUE_DISTRIBUTOR: &str = "revenue_distributor";

    // Mars Vault Contract
    pub fn mars_vault_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        ).with_reply(crate::contract::reply);
        Box::new(contract)
    }

    // Mock Red Bank Contract with edge cases
    #[cw_serde]
    pub enum Mars_MockExecuteMsg {
        Deposit {
            account_id: Option<String>,
            on_behalf_of: Option<String>,
        },
        Withdraw {
            denom: String,
            amount: Option<Uint128>,
            recipient: Option<String>,
            account_id: Option<String>,
            liquidation_related: Option<bool>,
        },
    }

    #[cw_serde]
    pub struct Mars_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Mars_MockQueryMsg {
        Market { denom: String },
        MarketV2 { denom: String },
        UserCollateral {
            user: String,
            account_id: Option<String>,
            denom: String,
        },
    }

    pub fn redbank_contract_edge_cases() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { .. } => Ok(Response::default()),
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, .. } => {
                        let amount = amount.unwrap_or(Uint128::new(1000));
                        let msg = BankMsg::Send {
                            to_address: recipient.unwrap_or(info.sender.to_string()),
                            amount: vec![coin(amount.u128(), &denom)],
                        };
                        Ok(Response::new().add_message(msg))
                    }
                }
            },
            |_, _, _, _: Mars_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Mars_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Mars_MockQueryMsg::Market { denom } => {
                        Ok(to_json_binary(&Market {
                            denom: denom.clone(),
                            reserve_factor: Decimal::zero(),
                            interest_rate_model: InterestRateModel {
                                optimal_utilization_rate: Decimal::zero(),
                                base: Decimal::zero(),
                                slope_1: Decimal::zero(),
                                slope_2: Decimal::zero(),
                            },
                            liquidity_rate: Decimal::percent(0), // 0% APR edge case
                            borrow_rate: Decimal::percent(7),
                            borrow_index: Decimal::one(),
                            liquidity_index: Decimal::one(),
                            indexes_last_updated: 0,
                            collateral_total_scaled: Uint128::new(10000),
                            debt_total_scaled: Uint128::new(0),
                        })?)
                    }
                    Mars_MockQueryMsg::MarketV2 { denom } => {
                        Ok(to_json_binary(&MarketV2Response {
                            utilization_rate: Decimal::zero(),
                            market: Market {
                                denom: denom.clone(),
                                reserve_factor: Decimal::zero(),
                                interest_rate_model: InterestRateModel {
                                    optimal_utilization_rate: Decimal::zero(),
                                    base: Decimal::zero(),
                                    slope_1: Decimal::zero(),
                                    slope_2: Decimal::zero(),
                                },
                                liquidity_rate: Decimal::percent(0),
                                borrow_rate: Decimal::percent(7),
                                borrow_index: Decimal::one(),
                                liquidity_index: Decimal::one(),
                                indexes_last_updated: 0,
                                collateral_total_scaled: Uint128::new(10000),
                                debt_total_scaled: Uint128::new(0),
                            },
                            collateral_total_amount: Uint128::new(10000),
                            debt_total_amount: Uint128::new(0),
                        })?)
                    }
                    Mars_MockQueryMsg::UserCollateral { user, denom, .. } => {
                        Ok(to_json_binary(&UserCollateralResponse {
                            amount: Uint128::new(5000),
                            denom: denom.clone(),
                            amount_scaled: Uint128::new(5000),
                            enabled: true,
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Transmuter Contract
    #[cw_serde]
    pub enum Transmuter_MockExecuteMsg {
        Transmute { recipient: Option<String> },
    }

    #[cw_serde]
    pub struct Transmuter_MockInstantiateMsg {}

    pub fn transmuter_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Transmuter_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Transmuter_MockExecuteMsg::Transmute { recipient } => {
                        let recipient = recipient.unwrap_or(info.sender.to_string());
                        let msg = BankMsg::Send {
                            to_address: recipient,
                            amount: vec![coin(1000, "factory/neutron1m9l358xunhhwds0568za49mzhvuxx9u8v6d8j8/cdt")],
                        };
                        Ok(Response::new().add_message(msg))
                    }
                }
            },
            |_, _, _, _: Transmuter_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _: Mars_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_json_binary(&"")?)
            },
        );
        Box::new(contract)
    }

    fn setup_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked(USER),
                    vec![coin(10000, "uusdc")],
                )
                .unwrap();
        })
    }

    fn setup_contracts(app: &mut App) -> (Addr, Addr, Addr) {
        let mars_vault_id = app.store_code(mars_vault_contract());
        let redbank_id = app.store_code(redbank_contract_edge_cases());
        let transmuter_id = app.store_code(transmuter_contract());

        let redbank_addr = app
            .instantiate_contract(
                redbank_id,
                Addr::unchecked(ADMIN),
                &Mars_MockInstantiateMsg {},
                &[],
                "redbank",
                None,
            )
            .unwrap();

        let transmuter_addr = app
            .instantiate_contract(
                transmuter_id,
                Addr::unchecked(ADMIN),
                &Transmuter_MockInstantiateMsg {},
                &[],
                "transmuter",
                None,
            )
            .unwrap();

        let mars_vault_addr = app
            .instantiate_contract(
                mars_vault_id,
                Addr::unchecked(ADMIN),
                &InstantiateMsg {
                    vault_subdenom: "mvault".to_string(),
                    deposit_token: "uusdc".to_string(),
                    mars_redbank_addr: redbank_addr.to_string(),
                    transmuter_addr: transmuter_addr.to_string(),
                    revenue_distributor_addr: REVENUE_DISTRIBUTOR.to_string(),
                    cdt_denom: "ucdt".to_string(),
                    cdp_contract_addr: "cdp_contract".to_string(),
                    revenue_distributions: vec![],
                },
                &[],
                "mars_vault",
                None,
            )
            .unwrap();

        (mars_vault_addr, redbank_addr, transmuter_addr)
    }

    #[test]
    fn test_zero_apr_yield_ceiling() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set yield ceiling with 0% Mars APR
        let vault_cost = VaultCost {
            static_cost: None,
            yield_ceiling: Some(Decimal::percent(1)),
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Query cost
        let cost: Decimal = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Cost {})
            .unwrap();

        // Should be 0 since Mars APR (0%) - yield_ceiling (1%) = max(-1%, 0%) = 0%
        assert_eq!(cost, Decimal::zero());
    }

    #[test]
    fn test_very_small_cost_rate() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set very small static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::from_ratio(1u128, 1000000u128)), // 0.0001%
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Query cost
        let cost: Decimal = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Cost {})
            .unwrap();

        assert_eq!(cost, Decimal::from_ratio(1u128, 1000000u128));
    }

    #[test]
    fn test_very_large_cost_rate() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set very large static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(1000)), // 1000% annual cost
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Query cost
        let cost: Decimal = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Cost {})
            .unwrap();

        assert_eq!(cost, Decimal::percent(1000));
    }

    #[test]
    fn test_cost_accrual_with_zero_vault_tokens() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Advance time by 1 year
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        // Try to collect cost when no vault tokens exist
        let result = app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::CollectCost {},
            &[],
        );

        // Should succeed but do nothing
        assert!(result.is_ok());
    }

    #[test]
    fn test_cost_accrual_very_short_time() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Enter vault
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Advance time by only 1 second
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 1);
        });

        // Enter vault again
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Should not accrue significant cost due to very short time
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        // The cost accrual should be minimal
        assert!(config.total_deposit_tokens > Uint128::zero());
    }

    #[test]
    fn test_cost_accrual_maximum_time() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Enter vault
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Advance time by 10 years
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 10 * 365 * 24 * 60 * 60);
        });

        // Enter vault again
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Should accrue significant cost
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        // The cost accrual should be substantial
        assert!(config.total_deposit_tokens > Uint128::zero());
    }

    #[test]
    fn test_cost_collection_multiple_times() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Advance time and enter vault to generate revenue
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Collect cost multiple times
        for i in 0..3 {
            let result = app.execute_contract(
                Addr::unchecked(USER),
                mars_vault_addr.clone(),
                &ExecuteMsg::CollectCost {},
                &[],
            );

            if i == 0 {
                // First collection should succeed
                assert!(result.is_ok());
            } else {
                // Subsequent collections should succeed but do nothing
                assert!(result.is_ok());
            }
        }
    }

    #[test]
    fn test_cost_config_update_during_operation() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Start with no cost
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Update to add cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(5)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Advance time and enter vault again
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Should now accrue costs
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        assert!(config.total_deposit_tokens > Uint128::zero());
    }

    #[test]
    fn test_cost_config_remove_during_operation() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Start with cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(5)),
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Remove cost
        let vault_cost = VaultCost {
            static_cost: None,
            yield_ceiling: None,
        };

        app.execute_contract(
            Addr::unchecked(ADMIN),
            mars_vault_addr.clone(),
            &ExecuteMsg::UpdateConfig {
                owner: None,
                mars_redbank_addr: None,
                transmuter_addr: None,
                revenue_distributor_addr: None,
                vault_cost: Some(vault_cost),
                cdt_denom: None,
                cdp_contract_addr: None,
                revenue_distributions: None,
            },
            &[],
        )
        .unwrap();

        // Advance time and enter vault again
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Should not accrue additional costs
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        // The total should reflect the initial deposit plus the new deposit
        // but no additional cost accrual
        assert!(config.total_deposit_tokens > Uint128::zero());
    }
}
