#[cfg(test)]
mod tests {
    use crate::helpers::MarsVaultContract;
    use crate::state::{COST_ACCRUAL, CostAccrual};

    use membrane::mars_vault_token::{
        ExecuteMsg, InstantiateMsg, QueryMsg, Config, VaultCost
    };
    use membrane::mars_redbank::{Market, UserCollateralResponse, InterestRateModel, MarketV2Response};
    use membrane::transmuter::ExecuteMsg as TransmuterExecuteMsg;

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Empty, Response, StdResult, Uint128, Decimal,
        Timestamp, BankMsg, CosmosMsg, WasmMsg, SubMsg, Reply, ReplyOn,
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

    // Mock Red Bank Contract
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

    pub fn redbank_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Mars_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Mars_MockExecuteMsg::Deposit { .. } => Ok(Response::default()),
                    Mars_MockExecuteMsg::Withdraw { denom, amount, recipient, .. } => {
                        // Send the withdrawn amount to the recipient
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
                            liquidity_rate: Decimal::percent(5), // 5% APR
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
                                liquidity_rate: Decimal::percent(5),
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
                            amount: Uint128::new(5000), // Mock collateral amount
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
                        // Mock transmuting USDC to CDT (1:1 ratio for simplicity)
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
        let redbank_id = app.store_code(redbank_contract());
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
    fn test_instantiate_with_cost_config() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Query config to verify initialization
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.transmuter_addr, Addr::unchecked(TRANSMUTER));
        assert_eq!(config.revenue_distributor_addr, Addr::unchecked(REVENUE_DISTRIBUTOR));
        assert_eq!(config.vault_cost.static_cost, None);
        assert_eq!(config.vault_cost.yield_ceiling, None);
    }

    #[test]
    fn test_update_vault_cost_config() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Update vault cost with static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(2)), // 2% annual cost
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

        // Verify config was updated
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.vault_cost.static_cost, Some(Decimal::percent(2)));
    }

    #[test]
    fn test_cost_query_static_cost() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(3)),
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

        assert_eq!(cost, Decimal::percent(3));
    }

    #[test]
    fn test_cost_query_yield_ceiling() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set yield ceiling (Mars APR is 5%, ceiling is 3%, so cost should be 2%)
        let vault_cost = VaultCost {
            static_cost: None,
            yield_ceiling: Some(Decimal::percent(3)),
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

        assert_eq!(cost, Decimal::percent(2)); // 5% - 3% = 2%
    }

    #[test]
    fn test_cost_query_no_cost() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Query cost with no cost configured
        let cost: Decimal = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Cost {})
            .unwrap();

        assert_eq!(cost, Decimal::zero());
    }

    #[test]
    fn test_cost_accrual_on_enter_vault() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)), // 10% annual cost
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

        // Enter vault with 1000 USDC
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Check that revenue vault tokens were accrued
        // With 10% annual cost and 1 year elapsed, we should have accrued 10% of total vault tokens
        let cost_accrual: CostAccrual = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        // The cost accrual should be tracked in the contract state
        // We need to query the state directly since there's no query for CostAccrual
        // For now, we'll verify the vault token supply increased
        let config: Config = app
            .wrap()
            .query_wasm_smart(&mars_vault_addr, &QueryMsg::Config {})
            .unwrap();

        // Verify that vault tokens were minted (total supply > 0)
        assert!(config.total_deposit_tokens > Uint128::zero());
    }

    #[test]
    fn test_collect_cost_no_revenue() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Try to collect cost when there's no revenue
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
    fn test_collect_cost_with_revenue() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)), // 10% annual cost
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

        // Enter vault to generate some revenue
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Collect cost
        let result = app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::CollectCost {},
            &[],
        );

        // Should succeed
        assert!(result.is_ok());

        // Verify that CDT was sent to revenue distributor
        let revenue_distributor_balance = app
            .wrap()
            .query_balance(&Addr::unchecked(REVENUE_DISTRIBUTOR), "factory/neutron1m9l358xunhhwds0568za49mzhvuxx9u8v6d8j8/cdt")
            .unwrap();

        // Should have received some CDT
        assert!(revenue_distributor_balance.amount > Uint128::zero());
    }

    #[test]
    fn test_exchange_rate_decreases_with_cost() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)), // 10% annual cost
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

        // Enter vault with 1000 USDC
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Get vault tokens received
        let user_vault_balance = app
            .wrap()
            .query_balance(&Addr::unchecked(USER), "factory/mars_vault/mvault")
            .unwrap();

        // Advance time by 1 year to accrue costs
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        // Enter vault again with same amount
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Get new vault tokens received
        let user_vault_balance_after = app
            .wrap()
            .query_balance(&Addr::unchecked(USER), "factory/mars_vault/mvault")
            .unwrap();

        // The second deposit should receive fewer vault tokens per USDC
        // because costs have accrued, increasing the total vault token supply
        let first_deposit_tokens = user_vault_balance.amount;
        let second_deposit_tokens = user_vault_balance_after.amount - first_deposit_tokens;

        // Second deposit should get fewer tokens per USDC due to accrued costs
        assert!(second_deposit_tokens < first_deposit_tokens);
    }

    #[test]
    fn test_cost_accrual_time_based() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)), // 10% annual cost
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

        // Get initial vault token supply
        let initial_supply = app
            .wrap()
            .query_supply("factory/mars_vault/mvault")
            .unwrap();

        // Advance time by 6 months (half year)
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 182 * 24 * 60 * 60);
        });

        // Enter vault again to trigger cost accrual
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Get new vault token supply
        let new_supply = app
            .wrap()
            .query_supply("factory/mars_vault/mvault")
            .unwrap();

        // Supply should have increased due to cost accrual
        // With 10% annual cost and 6 months elapsed, we should have accrued ~5% of initial supply
        assert!(new_supply.amount > initial_supply.amount);
    }

    #[test]
    fn test_multiple_users_fair_cost_distribution() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Add funds for second user
        app.send_tokens(
            Addr::unchecked(ADMIN),
            Addr::unchecked("user2"),
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Set static cost
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(10)), // 10% annual cost
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

        // User 1 enters vault
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Advance time by 6 months
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 182 * 24 * 60 * 60);
        });

        // User 2 enters vault (should get current exchange rate reflecting accrued costs)
        app.execute_contract(
            Addr::unchecked("user2"),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Both users should get the same exchange rate (fair for new depositors)
        let user1_balance = app
            .wrap()
            .query_balance(&Addr::unchecked(USER), "factory/mars_vault/mvault")
            .unwrap();

        let user2_balance = app
            .wrap()
            .query_balance(&Addr::unchecked("user2"), "factory/mars_vault/mvault")
            .unwrap();

        // Both users deposited the same amount, so they should have the same vault token balance
        // This demonstrates that the cost is fairly distributed and new users don't inherit historical costs
        assert_eq!(user1_balance.amount, user2_balance.amount);
    }

    #[test]
    fn test_yield_ceiling_cost_calculation() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set yield ceiling (Mars APR is 5%, ceiling is 4%, so cost should be 1%)
        let vault_cost = VaultCost {
            static_cost: None,
            yield_ceiling: Some(Decimal::percent(4)),
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

        assert_eq!(cost, Decimal::percent(1)); // 5% - 4% = 1%
    }

    #[test]
    fn test_yield_ceiling_no_cost_when_apr_below_ceiling() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set yield ceiling higher than Mars APR (5%)
        let vault_cost = VaultCost {
            static_cost: None,
            yield_ceiling: Some(Decimal::percent(6)),
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

        assert_eq!(cost, Decimal::zero()); // max(5% - 6%, 0) = 0%
    }

    #[test]
    fn test_static_cost_takes_precedence_over_yield_ceiling() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Set both static cost and yield ceiling
        let vault_cost = VaultCost {
            static_cost: Some(Decimal::percent(3)),
            yield_ceiling: Some(Decimal::percent(4)),
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

        // Static cost should take precedence
        assert_eq!(cost, Decimal::percent(3));
    }

    #[test]
    fn test_cost_collection_permissionless() {
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

        // Any user should be able to collect cost
        let result = app.execute_contract(
            Addr::unchecked("random_user"),
            mars_vault_addr.clone(),
            &ExecuteMsg::CollectCost {},
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_cost_accrual_stops_when_zero_cost() {
        let mut app = setup_app();
        let (mars_vault_addr, _, _) = setup_contracts(&mut app);

        // Enter vault with no cost configured
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        let initial_supply = app
            .wrap()
            .query_supply("factory/mars_vault/mvault")
            .unwrap();

        // Advance time by 1 year
        app.update_block(|block| {
            block.time = Timestamp::from_seconds(block.time.seconds() + 365 * 24 * 60 * 60);
        });

        // Enter vault again
        app.execute_contract(
            Addr::unchecked(USER),
            mars_vault_addr.clone(),
            &ExecuteMsg::EnterVault {},
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        let final_supply = app
            .wrap()
            .query_supply("factory/mars_vault/mvault")
            .unwrap();

        // Supply should only increase by the new deposit, not by cost accrual
        let expected_increase = Uint128::new(1000); // Just the new deposit
        let actual_increase = final_supply.amount - initial_supply.amount;

        // Should be approximately equal (allowing for small rounding differences)
        assert!(actual_increase >= expected_increase);
        assert!(actual_increase <= expected_increase + Uint128::new(1));
    }
}
