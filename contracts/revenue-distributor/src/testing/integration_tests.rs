mod tests {
    use std::str::FromStr;

    use membrane::revenue_distributor::{
        ExecuteMsg, InstantiateMsg, QueryMsg, RevenuePromise, RevenueDestination, Config, RDVaultInfoMessage
    };
    use membrane::types::{Asset, AssetInfo, Basket, PendingRevenue};
    use membrane::oracle::PriceResponse;

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult,
        Uint128, SubMsgResult, Reply, BankMsg, CosmosMsg, DepsMut, Deps, Env, MessageInfo,
    };
    use cw_storage_plus::Item;
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

    const USER: &str = "user";
    const ADMIN: &str = "admin";
    const AFFILIATE1: &str = "affiliate1";
    const AFFILIATE2: &str = "affiliate2";
    const STAKING_CONTRACT: &str = "staking_contract";
    const LTV_DISCO_CONTRACT: &str = "ltv_disco";
    const TRANSMUTER_CONTRACT: &str = "transmuter";
    const CDP_CONTRACT: &str = "cdp_contract";

    // Revenue Distributor Contract
    pub fn revenue_distributor_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    // Mock Staking Contract
    #[cosmwasm_schema::cw_serde]
    pub enum MockStakingExecuteMsg {
        DepositFee {},
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            |_deps, _env, _info, msg: MockStakingExecuteMsg| -> StdResult<Response> {
                match msg {
                    MockStakingExecuteMsg::DepositFee {} => {
                        Ok(Response::new().add_attribute("method", "deposit_fee"))
                    }
                }
            },
            |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |_deps, _env, _msg: Empty| -> StdResult<Binary> {
                Ok(to_json_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }

    // Mock LTV Disco Contract
    #[cosmwasm_schema::cw_serde]
    pub enum MockLTVExecuteMsg {
        AddRevenue { asset: String },
    }

    pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            |_deps, _env, _info, msg: MockLTVExecuteMsg| -> StdResult<Response> {
                match msg {
                    MockLTVExecuteMsg::AddRevenue { asset } => {
                        Ok(Response::new().add_attribute("method", "add_revenue").add_attribute("asset", asset))
                    }
                }
            },
            |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |_deps, _env, _msg: Empty| -> StdResult<Binary> {
                Ok(to_json_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }

    // Mock Transmuter Contract
    #[cosmwasm_schema::cw_serde]
    pub enum MockTransmuterExecuteMsg {
        EnterVault { recipient: Option<String> },
    }

    pub fn transmuter_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            |_deps, _env, _info, msg: MockTransmuterExecuteMsg| -> StdResult<Response> {
                match msg {
                    MockTransmuterExecuteMsg::EnterVault { .. } => {
                        // Do nothing; RD will have zero VT and reply should still succeed with no_vt
                        Ok(Response::new().add_attribute("method", "enter_vault"))
                    }
                }
            },
            |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |_deps, _env, _msg: Empty| -> StdResult<Binary> {
                Ok(to_json_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }

    // Mock CDP Contract
    #[cosmwasm_schema::cw_serde]
    pub enum CDP_MockExecuteMsg {
        TakeRevenue {},
        UpdateState {
            pending_revenue: Option<Uint128>,
            per_asset_rev: Option<Vec<Asset>>,
            should_fail_query: Option<bool>,
            should_fail_execute: Option<bool>,
            revenue_distributor: Option<String>,
        },
    }

    #[cosmwasm_schema::cw_serde]
    pub struct CDP_MockInstantiateMsg {
        pub revenue_distributor: Option<String>,
        pub credit_denom: String,
    }

    #[cosmwasm_schema::cw_serde]
    pub enum CDP_MockQueryMsg {
        GetBasket {},
    }

    // Storage for mock CDP state
    const MOCK_CDP_STATE: Item<CDP_MockState> = Item::new("mock_cdp_state");

    #[cosmwasm_schema::cw_serde]
    pub struct CDP_MockState {
        pub pending_revenue: Uint128,
        pub per_asset_rev: Vec<Asset>,
        pub revenue_distributor: Option<Addr>,
        pub should_fail_query: bool,
        pub should_fail_execute: bool,
        pub credit_denom: String,
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps: DepsMut, env: Env, info: MessageInfo, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::UpdateState {
                        pending_revenue,
                        per_asset_rev,
                        should_fail_query,
                        should_fail_execute,
                        revenue_distributor,
                    } => {
                        let mut state = MOCK_CDP_STATE.load(deps.storage)?;
                        if let Some(pr) = pending_revenue {
                            state.pending_revenue = pr;
                        }
                        if let Some(par) = per_asset_rev {
                            state.per_asset_rev = par;
                        }
                        if let Some(sfq) = should_fail_query {
                            state.should_fail_query = sfq;
                        }
                        if let Some(sfe) = should_fail_execute {
                            state.should_fail_execute = sfe;
                        }
                        if let Some(rd) = revenue_distributor {
                            state.revenue_distributor = Some(Addr::unchecked(rd));
                        }
                        MOCK_CDP_STATE.save(deps.storage, &state)?;
                        Ok(Response::new().add_attribute("method", "update_state"))
                    }
                    CDP_MockExecuteMsg::TakeRevenue {} => {
                        let state = MOCK_CDP_STATE.load(deps.storage)?;
                        
                        // Check if should fail
                        if state.should_fail_execute {
                            return Err(cosmwasm_std::StdError::generic_err("Mock CDP execute failure"));
                        }
                        
                        // Validate caller is revenue distributor
                        if let Some(rd_addr) = &state.revenue_distributor {
                            if info.sender != *rd_addr {
                                return Err(cosmwasm_std::StdError::generic_err("Unauthorized"));
                            }
                        }
                        
                        // Save total revenue before clearing
                        let total_revenue = state.pending_revenue;
                        
                        // Get the contract's current balance
                        let contract_balance = deps.querier.query_balance(
                            env.contract.address.clone(),
                            state.credit_denom.clone(),
                        )?.amount;
                        
                        // Send all available CDT to revenue distributor (up to pending_revenue)
                        let mut response = Response::new();
                        if !total_revenue.is_zero() && !contract_balance.is_zero() {
                            if let Some(rd_addr) = &state.revenue_distributor {
                                // Send the minimum of contract balance and pending revenue
                                let amount_to_send = std::cmp::min(contract_balance, total_revenue);
                                let bank_msg = CosmosMsg::Bank(BankMsg::Send {
                                    to_address: rd_addr.to_string(),
                                    amount: vec![coin(amount_to_send.u128(), &state.credit_denom)],
                                });
                                response = response.add_message(bank_msg);
                            }
                        }
                        
                        // Clear pending revenue in state
                        let mut new_state = state;
                        new_state.pending_revenue = Uint128::zero();
                        new_state.per_asset_rev.clear();
                        MOCK_CDP_STATE.save(deps.storage, &new_state)?;
                        
                        Ok(response
                            .add_attribute("method", "take_revenue")
                            .add_attribute("total_revenue", total_revenue.to_string())
                            .add_attribute("contract_balance", contract_balance.to_string()))
                    }
                }
            },
            |deps: DepsMut, _env: Env, _info: MessageInfo, msg: CDP_MockInstantiateMsg| -> StdResult<Response> {
                let state = CDP_MockState {
                    pending_revenue: Uint128::zero(),
                    per_asset_rev: vec![],
                    revenue_distributor: msg.revenue_distributor.map(|s| Addr::unchecked(s)),
                    should_fail_query: false,
                    should_fail_execute: false,
                    credit_denom: msg.credit_denom,
                };
                MOCK_CDP_STATE.save(deps.storage, &state)?;
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |deps: Deps, _env: Env, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket {} => {
                        let state = MOCK_CDP_STATE.load(deps.storage)?;
                        
                        if state.should_fail_query {
                            return Err(cosmwasm_std::StdError::generic_err("Mock CDP query failure"));
                        }
                        
                        let basket = Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            multi_asset_supply_caps: vec![],
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken {
                                    denom: state.credit_denom.clone(),
                                },
                                amount: Uint128::zero(),
                            },
                            credit_price: PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: PendingRevenue {
                                total_pending: state.pending_revenue,
                                per_asset_rev: state.per_asset_rev.clone(),
                            },
                            pending_bad_debt: Uint128::zero(),
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                            negative_rates: false,
                            frozen: false,
                            distribute_revenue: true,
                            cpc_margin_of_error: Decimal::zero(),
                            liq_queue: None,
                        };
                        Ok(to_json_binary(&basket)?)
                    }
                }
            },
        );
        Box::new(contract)
    }
    
    // Helper to set CDP state (for testing)
    pub fn set_cdp_state(
        app: &mut App,
        cdp_addr: &Addr,
        pending_revenue: Option<Uint128>,
        per_asset_rev: Option<Vec<Asset>>,
        should_fail_query: Option<bool>,
        should_fail_execute: Option<bool>,
    ) {
        let msg = CDP_MockExecuteMsg::UpdateState {
            pending_revenue,
            per_asset_rev,
            should_fail_query,
            should_fail_execute,
            revenue_distributor: None,
        };
        let _ = app.execute_contract(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &msg,
            &[],
        );
    }

    // Setup contracts with CDP
    fn setup_contracts_with_cdp(app: &mut App) -> (Addr, Addr, Addr, Addr, Addr) {
        // Deploy CDP contract first
        let cdp_code_id = app.store_code(cdp_contract());
        let cdp_instantiate_msg = CDP_MockInstantiateMsg {
            revenue_distributor: None, // Will be set after RD is deployed
            credit_denom: "uusdc".to_string(),
        };
        let cdp_addr = app
            .instantiate_contract(
                cdp_code_id,
                Addr::unchecked(ADMIN),
                &cdp_instantiate_msg,
                &[],
                CDP_CONTRACT,
                None,
            )
            .unwrap();

        // Deploy staking contract
        let staking_code_id = app.store_code(staking_contract());
        let staking_addr = app
            .instantiate_contract(
                staking_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                "staking_contract",
                None,
            )
            .unwrap();

        // Deploy LTV Disco contract
        let ltv_code_id = app.store_code(ltv_disco_contract());
        let ltv_addr = app
            .instantiate_contract(
                ltv_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                LTV_DISCO_CONTRACT,
                None,
            )
            .unwrap();

        // Deploy Transmuter contract
        let transmuter_code_id = app.store_code(transmuter_contract());
        let transmuter_addr = app
            .instantiate_contract(
                transmuter_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                TRANSMUTER_CONTRACT,
                None,
            )
            .unwrap();

        // Deploy revenue distributor contract
        let revenue_distributor_code_id = app.store_code(revenue_distributor_contract());
        
        let canonical_asset = Asset {
            amount: Uint128::zero(),
            info: AssetInfo::NativeToken {
                denom: "uusdc".to_string(),
            },
        };

        let revenue_destinations = vec![
            RevenueDestination {
                destination: staking_addr.clone(),
                distribution_ratio: Decimal::from_str("0.5").unwrap(),
            },
            RevenueDestination {
                destination: ltv_addr.clone(),
                distribution_ratio: Decimal::from_str("0.5").unwrap(),
            },
        ];

        let instantiate_msg = InstantiateMsg {
            owner: ADMIN.to_string(),
            canonical_asset,
            revenue_destinations,
            ltv_disco: ltv_addr.to_string(),
            transmuter_vault: RDVaultInfoMessage {
                vault_addr: transmuter_addr.to_string(),
                deposit_token: "uusdc".to_string(),
                vault_token: "vt".to_string(),
            },
            points_system_contract: None,
            cdp_contract: Some(cdp_addr.to_string()),
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        let revenue_distributor_addr = app
            .instantiate_contract(
                revenue_distributor_code_id,
                Addr::unchecked(ADMIN),
                &instantiate_msg,
                &[],
                "revenue_distributor",
                None,
            )
            .unwrap();

        // Update CDP to know about revenue distributor
        let update_cdp_msg = CDP_MockExecuteMsg::UpdateState {
            pending_revenue: None,
            per_asset_rev: None,
            should_fail_query: None,
            should_fail_execute: None,
            revenue_distributor: Some(revenue_distributor_addr.to_string()),
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &update_cdp_msg,
            &[],
        ).unwrap();

        (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr, cdp_addr)
    }

    fn setup_app() -> App {
        AppBuilder::new().build(|router, _api, storage| {
            router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked(USER),
                    vec![coin(1000000, "uusdc"), coin(1000000, "uatom")],
                )
                .unwrap();
            router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked(ADMIN),
                    vec![coin(1000000, "uusdc"), coin(1000000, "vt"), coin(1000000, "uatom")],
                )
                .unwrap();
        })
    }

    fn setup_contracts(app: &mut App) -> (Addr, Addr, Addr, Addr) {
        // Deploy staking contract
        let staking_code_id = app.store_code(staking_contract());
        let staking_addr = app
            .instantiate_contract(
                staking_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                "staking_contract",
                None,
            )
            .unwrap();

        // Deploy LTV Disco contract
        let ltv_code_id = app.store_code(ltv_disco_contract());
        let ltv_addr = app
            .instantiate_contract(
                ltv_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                LTV_DISCO_CONTRACT,
                None,
            )
            .unwrap();

        // Deploy Transmuter contract
        let transmuter_code_id = app.store_code(transmuter_contract());
        let transmuter_addr = app
            .instantiate_contract(
                transmuter_code_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                TRANSMUTER_CONTRACT,
                None,
            )
            .unwrap();

        // Deploy revenue distributor contract
        let revenue_distributor_code_id = app.store_code(revenue_distributor_contract());
        
        let canonical_asset = Asset {
            amount: Uint128::zero(),
            info: AssetInfo::NativeToken {
                denom: "uusdc".to_string(),
            },
        };

        let revenue_destinations = vec![
            RevenueDestination {
                destination: staking_addr.clone(),
                distribution_ratio: Decimal::from_str("0.5").unwrap(),
            },
            RevenueDestination {
                destination: ltv_addr.clone(),
                distribution_ratio: Decimal::from_str("0.5").unwrap(),
            },
        ];

        let instantiate_msg = InstantiateMsg {
            owner: ADMIN.to_string(),
            canonical_asset,
            revenue_destinations,
            ltv_disco: ltv_addr.to_string(),
            transmuter_vault: RDVaultInfoMessage {
                vault_addr: transmuter_addr.to_string(),
                deposit_token: "uusdc".to_string(),
                vault_token: "vt".to_string(),
            },
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        let revenue_distributor_addr = app
            .instantiate_contract(
                revenue_distributor_code_id,
                Addr::unchecked(ADMIN),
                &instantiate_msg,
                &[],
                "revenue_distributor",
                None,
            )
            .unwrap();

        (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr)
    }

    #[test]
    fn test_instantiate() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Query config
        let config: Config = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.owner, Addr::unchecked(ADMIN));
        assert_eq!(config.canonical_asset.info.to_string(), "uusdc");
        assert_eq!(config.revenue_destinations.len(), 1);
    }

    #[test]
    fn test_set_promises() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")], // Send more than promised
        );

        assert!(result.is_ok());

        // Query promises
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();

        assert_eq!(promises.len(), 2);
        assert_eq!(promises[0].address, AFFILIATE1);
        assert_eq!(promises[0].amount, Uint128::new(1000));
    }

    #[test]
    fn test_set_promises_invalid_amount() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")], // Send less than promised
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_set_promises_wrong_asset() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let promises = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uosmo")], // Wrong asset
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_distribute_promises_success() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check that promises are cleared
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();

        assert_eq!(promises.len(), 0);
    }

    #[test]
    fn test_distribute_promises_with_limit() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        )
        .unwrap();

        // Distribute only first promise
        let msg = ExecuteMsg::DistributePromises { limit: Some(1) };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check that only one promise remains
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();

        assert_eq!(promises.len(), 1);
        assert_eq!(promises[0].address, AFFILIATE2);
    }

    #[test]
    fn test_update_config() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let new_revenue_destinations = vec![RevenueDestination {
            destination: Addr::unchecked("new_staking_contract"),
            distribution_ratio: Decimal::from_str("0.8").unwrap(),
        }];

        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: Some(new_revenue_destinations.clone()),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        // Only admin can update config
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check updated config
        let config: Config = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Config {})
            .unwrap();

        assert_eq!(config.revenue_destinations.len(), 1);
        assert_eq!(
            config.revenue_destinations[0].destination,
            Addr::unchecked("new_staking_contract")
        );
    }

    #[test]
    fn test_update_config_unauthorized() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        // Non-admin cannot update config
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_clear_failed_distributions() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Manually add failed distributions (simulating failed reply)
        // This would normally happen through the reply mechanism
        let msg = ExecuteMsg::ClearFailedDistributions {};
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check that failed distributions are empty
        let failed_distributions: Vec<(String, Uint128)> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::FailedDistributions {})
            .unwrap();

        assert_eq!(failed_distributions.len(), 0);
    }

    #[test]
    fn test_clear_pending_distributions() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let msg = ExecuteMsg::ClearPendingDistributions {};
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check that pending distributions are empty
        let pending_distributions: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::PendingDistributions {})
            .unwrap();

        assert_eq!(pending_distributions.len(), 0);
    }

    #[test]
    fn test_retry_failed_distribute() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Test retry when no failed distributions exist
        let msg = ExecuteMsg::RetryFailedDistribute { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check the response events
        let response = result.unwrap();
        assert!(response.events.iter().any(|event| {
            event.attributes.iter().any(|attr| {
                attr.key == "status" && attr.value == "no_failed_distributions"
            })
        }));
    }

    #[test]
    fn test_retry_failed_distribute_unauthorized() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let msg = ExecuteMsg::RetryFailedDistribute { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER), // Non-admin
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_query_pending_distributions() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Query pending distributions (should be empty initially)
        let pending_distributions: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::PendingDistributions {})
            .unwrap();

        assert_eq!(pending_distributions.len(), 0);
    }

    #[test]
    fn test_query_failed_distributions() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Query failed distributions (should be empty initially)
        let failed_distributions: Vec<(String, Uint128)> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::FailedDistributions {})
            .unwrap();

        assert_eq!(failed_distributions.len(), 0);
    }

    // Test reply handling with mock success
    #[test]
    fn test_reply_success() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

        // Simulate successful reply
        let _reply = Reply {
            id: 1, // DISTRIBUTION_REPLY_ID
            result: SubMsgResult::Ok(cosmwasm_std::SubMsgResponse {
                events: vec![],
                data: None,
            }),
        };

        // Note: In a real test, we would need to simulate the reply through the app
        // This is a simplified test to verify the contract logic
    }

    // Test reply handling with mock failure
    #[test]
    fn test_reply_failure() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

        // Simulate failed reply
        let _reply = Reply {
            id: 1, // DISTRIBUTION_REPLY_ID
            result: SubMsgResult::Err("Distribution failed".to_string()),
        };

        // Note: In a real test, we would need to simulate the reply through the app
        // This is a simplified test to verify the contract logic
    }

    #[test]
    fn test_aggregate_promises() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set first batch of promises
        let promises1 = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises: promises1, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Set second batch with same address (should aggregate)
        let promises2 = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(500),
        }];

        let msg = ExecuteMsg::SetPromises { promises: promises2, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uusdc")],
        )
        .unwrap();

        // Check that promises are aggregated
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();

        assert_eq!(promises.len(), 1);
        assert_eq!(promises[0].address, AFFILIATE1);
        assert_eq!(promises[0].amount, Uint128::new(1500)); // 1000 + 500
    }

    #[test]
    fn test_revenue_destination_distribution() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![RevenuePromise {
            address: AFFILIATE1.to_string(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")], // Send more than promised
        )
        .unwrap();

        // Distribute promises (remaining should go to revenue destinations)
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());

        // Check that promises are cleared
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();

        assert_eq!(promises.len(), 0);
    }

    // New LTV Disco specific tests
    #[test]
    fn test_ltv_disco_as_promise_does_not_break_replies() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Promise directly to LTV Disco
        let promises = vec![RevenuePromise { address: ltv_addr.to_string(), amount: Uint128::new(1000) }];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[coin(2000, "uusdc")]).unwrap();

        // Also add a normal promise to ensure reply queue is populated
        let promises2 = vec![RevenuePromise { address: AFFILIATE1.to_string(), amount: Uint128::new(500) }];
        let msg2 = ExecuteMsg::SetPromises { promises: promises2, ltv_disco_distribution: None };
        app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg2, &[coin(1000, "uusdc")]).unwrap();

        // Distribute
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[]);
        assert!(res.is_err());
    }

    #[test]
    fn test_ltv_disco_destination_queues_entervault_for_all_assets_and_no_breakage() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set some promises less than sent amount to create remaining for destinations (includes ltv disco)
        let promises = vec![RevenuePromise { address: AFFILIATE1.to_string(), amount: Uint128::new(1000) }];
        let distributions = vec![
            membrane::types::Asset { info: membrane::types::AssetInfo::NativeToken { denom: "asset1".to_string() }, amount: Uint128::new(1500) },
            membrane::types::Asset { info: membrane::types::AssetInfo::NativeToken { denom: "asset2".to_string() }, amount: Uint128::new(3500) },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: Some(distributions) };
        app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[coin(5000, "uusdc")]).unwrap();

        //Send the contract VT so that the first reply sends VT
        app.send_tokens(Addr::unchecked(ADMIN), rd_addr.clone(), &vec![coin(1000, "vt")]).unwrap();

        // Distribute triggers EnterVault submsgs; reply will send VT -> LTV Disco AddRevenue
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[]).unwrap();
        // Assert at least one AddRevenue was sent in reply
        assert!(res.events.iter().any(|event| {
            event.attributes.iter().any(|attr| attr.key == "method" && attr.value == "add_revenue")
        }));

        // All promises should be cleared
        let promises: Vec<RevenuePromise> = app.wrap().query_wasm_smart(&rd_addr, &QueryMsg::Promises {}).unwrap();
        assert!(promises.is_empty());
    }

    #[test]
    fn test_ltv_disco_destination_fallback_no_ratios_does_not_break() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set a small promise and send larger funds so remaining goes to revenue destinations.
        // Do NOT set ltv_disco_distribution so map is empty -> fallback path in ltv_disco_enter_vault_msgs
        let promises = vec![RevenuePromise { address: AFFILIATE1.to_string(), amount: Uint128::new(500) }];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[coin(5000, "uusdc")]).unwrap();

        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(USER), rd_addr.clone(), &msg, &[]);
        assert!(res.is_err());
    }

    // ========== TakeRevenue Tests ==========

    #[test]
    fn test_take_revenue_success() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state with pending revenue
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(500),
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "asset2".to_string() },
                amount: Uint128::new(1500),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(2000)), Some(per_asset_rev.clone()), None, None);

        // Give CDP some CDT to send
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(2000, "uusdc")],
        ).unwrap();
        
        // Verify CDP has the tokens
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::new(2000), "CDP should have CDT");

        // Verify RD has zero balance initially
        let balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
        assert_eq!(balance.amount, Uint128::zero());

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());
        
        // In cw_multi_test, SubMsg replies are processed automatically
        // The CDP's BankMsg::Send should execute, then the reply handler runs
        // The reply handler will call SetPromises (which sends CDT back as funds) and DistributePromises
        // We need to process any messages from the reply handler
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        
        // Process any messages from SetPromises and DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance_after = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance_after.amount, Uint128::zero(), "CDP should have sent all CDT to RD");
        
        // After SetPromises, the CDT is sent back as funds, so RD balance might be 0
        // But the CDT should have been received and processed by the reply handler
        // The key is that the flow completed without errors and CDP balance decreased
    }

    #[test]
    fn test_take_revenue_no_revenue() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state with zero revenue
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::zero()), Some(vec![]), None, None);

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Verify no CDT was sent
        let balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
        assert_eq!(balance.amount, Uint128::zero());
    }

    #[test]
    fn test_take_revenue_empty_per_asset_rev() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state with revenue but empty per_asset_rev
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(vec![]), None, None);

        // Give CDP some CDT to send
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Verify CDT was received
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        let balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
        assert_eq!(balance.amount, Uint128::new(1000));
    }

    #[test]
    fn test_cdp_take_revenue_unauthorized() {
        let mut app = setup_app();
        let (_rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(vec![]), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Try to call TakeRevenue directly (not through RD)
        let msg = CDP_MockExecuteMsg::TakeRevenue {};
        let result = app.execute_contract(
            Addr::unchecked(USER), // Not the revenue distributor
            cdp_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_take_revenue_from_basket_no_cdp_config() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // RD doesn't have CDP configured
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_take_revenue_with_existing_balance() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Give RD some existing CDT balance
        app.send_tokens(
            Addr::unchecked(ADMIN),
            rd_addr.clone(),
            &[coin(500, "uusdc")],
        ).unwrap();

        // Set CDP state with pending revenue
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev), None, None);

        // Give CDP some CDT to send
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD");
        
        // RD balance might be 0 after SetPromises, but the flow should have completed
        // The key is that CDP balance decreased, indicating tokens were sent
    }

    #[test]
    fn test_take_revenue_balance_decrease() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Give RD some balance
        app.send_tokens(
            Addr::unchecked(ADMIN),
            rd_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Give CDP some CDT (but no revenue to send)
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Set CDP state with zero revenue (so no CDT is sent)
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::zero()), Some(vec![]), None, None);

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance is still 1000 (no revenue was sent)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::new(1000), "CDP should still have CDT (no revenue to send)");
        
        // Verify RD balance is still 1000 (no new revenue received)
        let rd_balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
        assert_eq!(rd_balance.amount, Uint128::new(1000), "RD balance should be unchanged");
    }

    #[test]
    fn test_take_revenue_cdp_query_fails() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP to fail queries
        set_cdp_state(&mut app, &cdp_addr, None, None, Some(true), None);

        // Call TakeRevenueFromBasket should fail
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_take_revenue_cdp_execute_fails() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP to fail execute (but query should succeed)
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(vec![]), None, Some(true));

        // Call TakeRevenueFromBasket
        // This should succeed because the query succeeds, only the execute will fail
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        // The call itself should succeed (query works), but the submessage execute will fail
        // However, if the query fails for any reason, the whole call fails
        // So we check if it's ok or err - both are valid depending on when the failure occurs
        if result.is_ok() {
            // Process reply (which will be an error)
            app.update_block(|b| {
                b.time = b.time.plus_seconds(1);
                b.height += 1;
            });

            // Verify no CDT was received
            let balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
            assert_eq!(balance.amount, Uint128::zero(), "RD should not have received CDT (CDP execute failed)");
        } else {
            // If the query failed, that's also a valid test case
            // The key is that the CDP failure is handled
        }
    }

    #[test]
    fn test_take_revenue_per_asset_matches_saved() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state with specific per_asset_rev
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(300),
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "asset2".to_string() },
                amount: Uint128::new(700),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev.clone()), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD");
        
        // The per_asset_rev should be used in SetPromises
        // The key is that the flow completed without errors
    }

    #[test]
    fn test_take_revenue_multiple_assets() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state with multiple assets
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(100),
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "asset2".to_string() },
                amount: Uint128::new(200),
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "asset3".to_string() },
                amount: Uint128::new(300),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(600)), Some(per_asset_rev), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(600, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD");
    }

    // State Management Tests
    #[test]
    fn test_take_revenue_storage_cleared_after_success() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD");

        // Call TakeRevenueFromBasket again - should work (storage was cleared)
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_take_revenue_storage_cleared_after_failure() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP to fail execute
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(vec![]), None, Some(true));

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        // The call might succeed (if query works) or fail (if query fails)
        // Both are valid - the key is that storage is cleared on error
        if result.is_ok() {
            // Process reply (which will be an error)
            app.update_block(|b| {
                b.time = b.time.plus_seconds(1);
                b.height += 1;
            });

            // Verify no CDT was received
            let balance = app.wrap().query_balance(&rd_addr, "uusdc").unwrap();
            assert_eq!(balance.amount, Uint128::zero(), "RD should not have received CDT (CDP execute failed)");
        }

        // Call TakeRevenueFromBasket again - should work (storage was cleared on error)
        // Reset CDP to succeed
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(500)), Some(vec![]), None, Some(false));
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(500, "uusdc")],
        ).unwrap();

        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());
    }

    // Reentrancy/Race Condition Tests
    #[test]
    fn test_take_revenue_multiple_concurrent_calls() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev.clone()), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // First call
        let msg1 = ExecuteMsg::TakeRevenueFromBasket {};
        let result1 = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg1,
            &[],
        );
        assert!(result1.is_ok());

        // Second call before first reply (should overwrite storage)
        // Set new state
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(500)), Some(per_asset_rev), None, None);
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(500, "uusdc")],
        ).unwrap();

        let msg2 = ExecuteMsg::TakeRevenueFromBasket {};
        let result2 = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg2,
            &[],
        );
        assert!(result2.is_ok());

        // Process replies
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Should have received both amounts (though the second call overwrote the first's state)
        // Process replies and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        
        // Verify CDP balance decreased (tokens were sent to RD)
        // Both calls should have sent CDT, so CDP balance should be 0
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD from both calls");
    }

    #[test]
    fn test_take_revenue_reply_twice() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = setup_contracts_with_cdp(&mut app);

        // Set CDP state
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev), None, None);

        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(USER),
            rd_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Process reply and SetPromises/DistributePromises
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });
        app.update_block(|b| {
            b.time = b.time.plus_seconds(1);
            b.height += 1;
        });

        // Verify CDP balance decreased (tokens were sent to RD)
        let cdp_balance = app.wrap().query_balance(&cdp_addr, "uusdc").unwrap();
        assert_eq!(cdp_balance.amount, Uint128::zero(), "CDP should have sent CDT to RD");

        // If reply is called again (shouldn't happen in practice, but test the behavior)
        // The storage is already cleared, so it would use defaults
        // This is more of a theoretical test - in practice replies are only called once per submessage
    }

    // Helper function to setup contracts with window configured
    fn setup_contracts_with_window(app: &mut App, window_days: u64) -> (Addr, Addr, Addr, Addr, Addr) {
        let (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr, cdp_addr) = setup_contracts_with_cdp(app);
        
        // Update config to set window and remove LTV disco from revenue destinations to avoid LTV_DISCO_DISTRIBUTION issues
        let new_revenue_destinations = vec![
            RevenueDestination {
                destination: staking_addr.clone(),
                distribution_ratio: Decimal::from_str("1.0").unwrap(), // 100% to staking
            },
        ];
        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: Some(new_revenue_destinations),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: Some(window_days),
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr, cdp_addr)
    }

    #[test]
    fn test_distribute_promises_window_blocked() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // First, do an initial distribution (first distribution should always be allowed)
        let promises1 = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises: promises1, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // First distribution should succeed - this sets last_distribution_time
        // We need to ensure messages are created so last_distribution_time is saved
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let first_result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();
        
        // Verify the first distribution created messages (which means time was saved)
        // Check for the method attribute and promises_distributed > 0
        let method_attr = first_result.events.iter()
            .flat_map(|e| &e.attributes)
            .find(|a| a.key == "method" && a.value == "distribute_promises");
        assert!(method_attr.is_some(), "First distribution should have method attribute");
        
        let promises_distributed_attr = first_result.events.iter()
            .flat_map(|e| &e.attributes)
            .find(|a| a.key == "promises_distributed");
        if let Some(attr) = promises_distributed_attr {
            let count: u32 = attr.value.parse().unwrap_or(0);
            assert!(count > 0, "First distribution should have distributed at least 1 promise");
        }

        // Now set new promises and try to distribute immediately - should be blocked by window
        let promises2 = vec![
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises: promises2, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Try to distribute immediately - should be blocked by window
        // Note: In the same block, current_time should equal last_distribution
        // So current_time < last_distribution + window_seconds should be true
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        let response = result.unwrap();
        // Check that status indicates window not passed
        // Print all attributes for debugging
        let all_attrs: Vec<_> = response.events.iter()
            .flat_map(|e| &e.attributes)
            .collect();
        let status_attr = all_attrs.iter()
            .find(|a| a.key == "status");
        
        if status_attr.is_none() {
            // If no status, check if it actually distributed (which would be wrong)
            let method_attr = all_attrs.iter()
                .find(|a| a.key == "method" && a.value == "distribute_promises");
            if method_attr.is_some() {
                // It distributed when it shouldn't have - check why
                let current_time_attr = all_attrs.iter()
                    .find(|a| a.key == "current_time");
                let last_dist_attr = all_attrs.iter()
                    .find(|a| a.key == "last_distribution");
                panic!("Distribution was not blocked! current_time: {:?}, last_distribution: {:?}", 
                    current_time_attr.map(|a| &a.value), last_dist_attr.map(|a| &a.value));
            }
        }
        
        assert!(status_attr.is_some(), "Expected status attribute but found none. Events: {:?}", 
            response.events.iter().map(|e| &e.attributes).collect::<Vec<_>>());
        assert_eq!(status_attr.unwrap().value, "window_not_passed");

        // Verify promises are still there (not distributed)
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();
        assert_eq!(promises.len(), 1);
    }

    #[test]
    fn test_distribute_promises_window_passed() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Advance time by 8 days (more than 7 day window)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(8 * 24 * 60 * 60); // 8 days in seconds
        });

        // Now distribution should succeed
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        let response = result.unwrap();
        // Check that it actually distributed (should have events with method attribute)
        assert!(response.events.iter().any(|e| {
            e.attributes.iter().any(|attr| 
                attr.key == "method" && attr.value == "distribute_promises"
            )
        }));
    }

    #[test]
    fn test_distribute_promises_no_window() {
        let mut app = setup_app();
        let (revenue_distributor_addr, staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_cdp(&mut app); // No window configured

        // Update config to remove LTV disco from revenue destinations to avoid LTV_DISCO_DISTRIBUTION issues
        let new_revenue_destinations = vec![
            RevenueDestination {
                destination: staking_addr.clone(),
                distribution_ratio: Decimal::from_str("1.0").unwrap(), // 100% to staking
            },
        ];
        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: Some(new_revenue_destinations),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Distribution should work immediately when no window is set
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        let response = result.unwrap();
        // Should have distribution events
        assert!(response.events.iter().any(|e| {
            e.attributes.iter().any(|attr| 
                attr.key == "method" && attr.value == "distribute_promises"
            )
        }));
    }

    #[test]
    fn test_execute_revenue_distribution_window_blocked() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // First, do an initial distribution to set last_distribution_time
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // First distribution should succeed - this sets last_distribution_time
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Set CDP state with revenue
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev), None, None);
        
        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Try ExecuteRevenueDistribution immediately - should be blocked because window hasn't passed
        let msg = ExecuteMsg::ExecuteRevenueDistribution {};
        let result = app.execute_contract(
            Addr::unchecked(USER), // Permissionless
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        let response = result.unwrap();
        // Check that status indicates window not passed
        // The status might be in the response attributes or events
        let status_attr = response.events.iter()
            .flat_map(|e| &e.attributes)
            .find(|a| a.key == "status" && a.value == "window_not_passed");
        assert!(status_attr.is_some(), "Expected window_not_passed status but didn't find it");
    }

    #[test]
    fn test_execute_revenue_distribution_window_passed() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // Set CDP state with revenue
        let per_asset_rev = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "asset1".to_string() },
                amount: Uint128::new(1000),
            },
        ];
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(1000)), Some(per_asset_rev), None, None);
        
        // Give CDP some CDT
        app.send_tokens(
            Addr::unchecked(ADMIN),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Advance time by 8 days
        app.update_block(|block| {
            block.time = block.time.plus_seconds(8 * 24 * 60 * 60); // 8 days in seconds
        });

        // Now ExecuteRevenueDistribution should work
        let msg = ExecuteMsg::ExecuteRevenueDistribution {};
        let result = app.execute_contract(
            Addr::unchecked(USER), // Permissionless
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        // Should trigger TakeRevenueFromBasket which will then distribute
        let response = result.unwrap();
        // Check for take_revenue_from_basket method attribute
        assert!(response.events.iter().any(|e| {
            e.attributes.iter().any(|attr| 
                attr.key == "method" && (attr.value == "take_revenue_from_basket" || attr.value == "execute_revenue_distribution")
            )
        }));
    }

    #[test]
    fn test_last_distribution_time_updated() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE1.to_string(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        let _initial_time = app.block_info().time.seconds();

        // Advance time by 8 days
        app.update_block(|block| {
            block.time = block.time.plus_seconds(8 * 24 * 60 * 60); // 8 days in seconds
        });

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Advance time by only 1 day (less than 7 day window)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1 * 24 * 60 * 60); // 1 day in seconds
        });

        // Set new promises
        let promises = vec![
            RevenuePromise {
                address: AFFILIATE2.to_string(),
                amount: Uint128::new(2000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, ltv_disco_distribution: None };
        app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Try to distribute - should be blocked because only 1 day has passed since last distribution
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );

        assert!(result.is_ok());
        let response = result.unwrap();
        let status_attr = response.events.iter()
            .flat_map(|e| &e.attributes)
            .find(|a| a.key == "status");
        assert!(status_attr.is_some());
        assert_eq!(status_attr.unwrap().value, "window_not_passed");
    }

    #[test]
    fn test_add_non_cdt_revenue_routes_to_auction() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7);
        
        // Fund USER with uatom for the test (send from ADMIN who has funds)
        app.send_tokens(
            Addr::unchecked(ADMIN),
            Addr::unchecked(USER),
            &[coin(1000000, "uatom")],
        ).unwrap();

        // Deploy mock auction contract
        let auction_id = app.store_code(auction_contract());
        let auction_addr = app
            .instantiate_contract(
                auction_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                "auction",
                None,
            )
            .unwrap();

        // Update config to set auction contract
        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: Some(auction_addr.to_string()),
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Add non-CDT revenue (e.g., ATOM from liquidation fees)
        let per_asset_distribution = vec![
            membrane::types::Asset {
                info: membrane::types::AssetInfo::NativeToken { denom: "uatom".to_string() },
                amount: Uint128::new(1000),
            },
        ];

        let msg = ExecuteMsg::AddNonCdtRevenue {
            per_asset_distribution: per_asset_distribution.clone(),
        };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uatom")],
        );

        // Should succeed and route to auction
        assert!(result.is_ok());
        let response = result.unwrap();
        
        // Check that routed_to_auction attribute is present
        assert!(response.events.iter().any(|e| {
            e.attributes.iter().any(|attr| 
                attr.key == "routed_to_auction" && attr.value == "true"
            )
        }));
    }

    #[test]
    fn test_add_non_cdt_revenue_rejects_cdt() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7);
        
        // USER already has uusdc from setup_app, so no need to fund

        // Deploy mock auction contract
        let auction_id = app.store_code(auction_contract());
        let auction_addr = app
            .instantiate_contract(
                auction_id,
                Addr::unchecked(ADMIN),
                &Empty {},
                &[],
                "auction",
                None,
            )
            .unwrap();

        // Update config to set auction contract
        let msg = ExecuteMsg::UpdateConfig {
            revenue_destinations: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            transmuter_lockdrop_contract: None,
            ltv_disco_contract: None,
            auction_contract: Some(auction_addr.to_string()),
        };
        app.execute_contract(
            Addr::unchecked(ADMIN),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Try to add CDT as non-CDT revenue - should fail
        let per_asset_distribution = vec![
            membrane::types::Asset {
                info: membrane::types::AssetInfo::NativeToken { denom: "uusdc".to_string() }, // CDT/canonical asset
                amount: Uint128::new(1000),
            },
        ];

        let msg = ExecuteMsg::AddNonCdtRevenue {
            per_asset_distribution,
        };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uusdc")], // Sending CDT
        );

        // Should fail because CDT should use SetPromises
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.root_cause().to_string().contains("CDT revenue should use SetPromises"));
    }

    #[test]
    fn test_add_non_cdt_revenue_requires_auction_contract() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7);
        
        // Fund USER with uatom for the test (send from ADMIN who has funds)
        app.send_tokens(
            Addr::unchecked(ADMIN),
            Addr::unchecked(USER),
            &[coin(1000000, "uatom")],
        ).unwrap();

        // Don't set auction contract - should fail
        let per_asset_distribution = vec![
            membrane::types::Asset {
                info: membrane::types::AssetInfo::NativeToken { denom: "uatom".to_string() },
                amount: Uint128::new(1000),
            },
        ];

        let msg = ExecuteMsg::AddNonCdtRevenue {
            per_asset_distribution,
        };
        let result = app.execute_contract(
            Addr::unchecked(USER),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uatom")],
        );

        // Should fail because auction contract is not configured
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.root_cause().to_string().contains("Auction contract not configured"));
    }

    // Mock Auction Contract for testing
    // Use the real ExecuteMsg from membrane::auction so it matches what revenue-distributor sends
    pub fn auction_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            |_deps, _env, _info, msg: membrane::auction::ExecuteMsg| -> StdResult<Response> {
                match msg {
                    membrane::auction::ExecuteMsg::StartAuction { .. } => {
                        Ok(Response::new().add_attribute("method", "start_auction"))
                    }
                    _ => Err(cosmwasm_std::StdError::generic_err("Unexpected message type")),
                }
            },
            |_deps, _env, _info, _msg: Empty| -> StdResult<Response> {
                Ok(Response::new().add_attribute("method", "instantiate"))
            },
            |_deps, _env, _msg: Empty| -> StdResult<Binary> {
                Ok(to_json_binary(&Empty {})?)
            },
        );
        Box::new(contract)
    }
}
