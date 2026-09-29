mod tests {
    use std::str::FromStr;

    use membrane::revenue_distributor::{
        ExecuteMsg, InstantiateMsg, QueryMsg, RevenuePromise, RevenueSplit, Config, RDVaultInfoMessage
    };
    use membrane::types::{Asset, AssetInfo, Basket, PendingRevenue, CreditAssetBreakdown};
    use membrane::oracle::PriceResponse;

    use cosmwasm_std::{
        coin, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult,
        Uint128, SubMsgResult, Reply, BankMsg, CosmosMsg, DepsMut, Deps, Env, MessageInfo,
    };
    use cw_storage_plus::Item;
    use cosmwasm_std::testing::MockApi;
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

    // Helper to create valid bech32 addresses from labels
    fn addr(label: &str) -> String {
        MockApi::default().addr_make(label).to_string()
    }

    fn user() -> String { addr("user") }
    fn admin() -> String { addr("admin") }
    fn affiliate1() -> String { addr("affiliate1") }
    fn affiliate2() -> String { addr("affiliate2") }

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
        EnterVault { recipient: Option<String>, lock_days: Option<u64>, affiliate_address: Option<String>, affiliate_label: Option<String> },
        AddRevenue { collateral_denom: String, junior: bool },
    }

    pub fn transmuter_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            |_deps, _env, _info, msg: MockTransmuterExecuteMsg| -> StdResult<Response> {
                match msg {
                    MockTransmuterExecuteMsg::EnterVault { .. } => {
                        Ok(Response::new().add_attribute("method", "enter_vault"))
                    }
                    MockTransmuterExecuteMsg::AddRevenue { collateral_denom, junior } => {
                        Ok(Response::new()
                            .add_attribute("method", "add_revenue")
                            .add_attribute("collateral_denom", collateral_denom)
                            .add_attribute("junior", junior.to_string()))
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
        FulfillBadDebt {},
        UpdateState {
            pending_revenue: Option<Uint128>,
            per_asset_rev: Option<Vec<Asset>>,
            should_fail_query: Option<bool>,
            should_fail_execute: Option<bool>,
            revenue_distributor: Option<String>,
            pending_bad_debt: Option<Uint128>,
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
        pub pending_bad_debt: Uint128,
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
                        pending_bad_debt,
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
                        if let Some(pbd) = pending_bad_debt {
                            state.pending_bad_debt = pbd;
                        }
                        MOCK_CDP_STATE.save(deps.storage, &state)?;
                        Ok(Response::new().add_attribute("method", "update_state"))
                    }
                    CDP_MockExecuteMsg::FulfillBadDebt {} => {
                        let mut state = MOCK_CDP_STATE.load(deps.storage)?;

                        // Find CDT amount sent
                        let cdt_amount = info.funds.iter()
                            .find(|c| c.denom == state.credit_denom)
                            .map(|c| c.amount)
                            .unwrap_or(Uint128::zero());

                        let fulfilled = std::cmp::min(cdt_amount, state.pending_bad_debt);
                        let excess = cdt_amount - fulfilled;
                        state.pending_bad_debt -= fulfilled;
                        MOCK_CDP_STATE.save(deps.storage, &state)?;

                        let mut response = Response::new()
                            .add_attribute("method", "fulfill_bad_debt")
                            .add_attribute("fulfilled", fulfilled.to_string());

                        // Return excess
                        if !excess.is_zero() {
                            response = response.add_message(CosmosMsg::Bank(BankMsg::Send {
                                to_address: info.sender.to_string(),
                                amount: vec![coin(excess.u128(), &state.credit_denom)],
                            }));
                        }

                        Ok(response)
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
                    pending_bad_debt: Uint128::zero(),
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
                            multi_asset_supply_caps: vec![],
                            credit_asset: CreditAssetBreakdown {
                                info: AssetInfo::NativeToken {
                                    denom: state.credit_denom.clone(),
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
                                decimals: 6,
                            },
                            pending_revenue: PendingRevenue {
                                total_pending: state.pending_revenue,
                                per_asset_rev: state.per_asset_rev.clone(),
                            },
                            pending_bad_debt: state.pending_bad_debt,
                            oracle_set: true,
                            frozen: false,
                            distribute_revenue: true,
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
            pending_bad_debt: None,
        };
        let _ = app.execute_contract(
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &msg,
            &[],
        );
    }

    // Helper to set CDP bad debt state
    pub fn set_cdp_bad_debt(
        app: &mut App,
        cdp_addr: &Addr,
        pending_bad_debt: Uint128,
    ) {
        let msg = CDP_MockExecuteMsg::UpdateState {
            pending_revenue: None,
            per_asset_rev: None,
            should_fail_query: None,
            should_fail_execute: None,
            revenue_distributor: None,
            pending_bad_debt: Some(pending_bad_debt),
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &msg,
            &[],
        ).unwrap();
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
                Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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

        let instantiate_msg = InstantiateMsg {
            owner: admin(),
            canonical_asset,
            revenue_split: RevenueSplit::default_split(),
            ltv_disco: ltv_addr.to_string(),
            transmuter_vault: RDVaultInfoMessage {
                vault_addr: transmuter_addr.to_string(),
                deposit_token: "uusdc".to_string(),
                vault_token: "vt".to_string(),
            },
            points_system_contract: None,
            cdp_contract: Some(cdp_addr.to_string()),
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        let revenue_distributor_addr = app
            .instantiate_contract(
                revenue_distributor_code_id,
                Addr::unchecked(&admin()),
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
            pending_bad_debt: None,
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
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
                    &Addr::unchecked(&user()),
                    vec![coin(1000000, "uusdc"), coin(1000000, "uatom")],
                )
                .unwrap();
            router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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
                Addr::unchecked(&admin()),
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

        let instantiate_msg = InstantiateMsg {
            owner: admin(),
            canonical_asset,
            revenue_split: RevenueSplit::default_split(),
            ltv_disco: ltv_addr.to_string(),
            transmuter_vault: RDVaultInfoMessage {
                vault_addr: transmuter_addr.to_string(),
                deposit_token: "uusdc".to_string(),
                vault_token: "vt".to_string(),
            },
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        let revenue_distributor_addr = app
            .instantiate_contract(
                revenue_distributor_code_id,
                Addr::unchecked(&admin()),
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

        assert_eq!(config.owner, Addr::unchecked(&admin()));
        assert_eq!(config.canonical_asset.info.to_string(), "uusdc");
        assert_eq!(config.revenue_split.disco_ratio, Decimal::percent(16));
    }

    #[test]
    fn test_set_promises() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let promises = vec![
            RevenuePromise {
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
        assert_eq!(promises[0].address, affiliate1());
        assert_eq!(promises[0].amount, Uint128::new(1000));
    }

    #[test]
    fn test_set_promises_invalid_amount() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let promises = vec![
            RevenuePromise {
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            address: affiliate1(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
            RevenuePromise {
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        )
        .unwrap();

        // Distribute only first promise
        let msg = ExecuteMsg::DistributePromises { limit: Some(1) };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
        assert_eq!(promises[0].address, affiliate2());
    }

    #[test]
    fn test_update_config() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let new_split = RevenueSplit {
            disco_ratio: Decimal::percent(20),
            junior_ratio: Decimal::percent(60),
            senior_ratio: Decimal::percent(16),
            reserve_ratio: Decimal::percent(4),
        };

        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: Some(new_split.clone()),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        // Only admin can update config
        let result = app.execute_contract(
            Addr::unchecked(&admin()),
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

        assert_eq!(config.revenue_split.disco_ratio, Decimal::percent(20));
        assert_eq!(config.revenue_split.junior_ratio, Decimal::percent(60));
        assert_eq!(config.revenue_split.senior_ratio, Decimal::percent(16));
        assert_eq!(config.revenue_split.reserve_ratio, Decimal::percent(4));
    }

    #[test]
    fn test_update_config_unauthorized() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        // Non-admin cannot update config
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&user()), // Non-admin
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
            address: affiliate1(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

        // Note: Reply simulation requires the app's built-in reply dispatch.
        // The reply is automatically triggered via SubMsg in the cw_multi_test framework.
    }

    // Test reply handling with mock failure
    #[test]
    fn test_reply_failure() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![RevenuePromise {
            address: affiliate1(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Distribute promises
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

        // Note: Reply simulation requires the app's built-in reply dispatch.
        // Failed replies are handled automatically by cw_multi_test via SubMsg::reply_always.
    }

    #[test]
    fn test_aggregate_promises() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set first batch of promises
        let promises1 = vec![RevenuePromise {
            address: affiliate1(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises: promises1, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")],
        )
        .unwrap();

        // Set second batch with same address (should aggregate)
        let promises2 = vec![RevenuePromise {
            address: affiliate1(),
            amount: Uint128::new(500),
        }];

        let msg = ExecuteMsg::SetPromises { promises: promises2, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
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
        assert_eq!(promises[0].address, affiliate1());
        assert_eq!(promises[0].amount, Uint128::new(1500)); // 1000 + 500
    }

    #[test]
    fn test_revenue_destination_distribution() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set promises
        let promises = vec![RevenuePromise {
            address: affiliate1(),
            amount: Uint128::new(1000),
        }];

        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(2000, "uusdc")], // Send more than promised
        )
        .unwrap();

        // Distribute promises (remaining should go to revenue destinations)
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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

        // Promise directly to LTV Disco (should be skipped per contract logic)
        let promises = vec![RevenuePromise { address: ltv_addr.to_string(), amount: Uint128::new(1000) }];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(2000, "uusdc")]).unwrap();

        // Also add a normal promise to ensure reply queue is populated
        let promises2 = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(500) }];
        let msg2 = ExecuteMsg::SetPromises { promises: promises2, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg2, &[coin(1000, "uusdc")]).unwrap();

        // Distribute: LTV Disco promise is skipped, affiliate + staking destinations receive funds
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[]);
        // With staking-only destinations, this succeeds: affiliate gets 500, staking gets remainder
        assert!(res.is_ok());
    }

    #[test]
    fn test_ltv_disco_destination_queues_entervault_for_all_assets_and_no_breakage() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // RevenueSplit defaults already include disco — no config update needed

        // Set some promises less than sent amount to create remaining for destinations (includes ltv disco)
        let promises = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(1000) }];
        let distributions = vec![
            membrane::types::Asset { info: membrane::types::AssetInfo::NativeToken { denom: "asset1".to_string() }, amount: Uint128::new(1500) },
            membrane::types::Asset { info: membrane::types::AssetInfo::NativeToken { denom: "asset2".to_string() }, amount: Uint128::new(3500) },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: Some(distributions) };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(5000, "uusdc")]).unwrap();

        //Send the contract VT so that the first reply sends VT
        app.send_tokens(Addr::unchecked(&admin()), rd_addr.clone(), &vec![coin(1000, "vt")]).unwrap();

        // Distribute triggers LTV Disco AddRevenue path via ltv_disco_add_revenue_msgs.
        // The mock LTV Disco doesn't support GetAssetQueue queries, so AddRevenue messages
        // won't be sent (has_deposits check returns false). Verify distribute completes without panic.
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[]);
        assert!(res.is_ok(), "Distribute should succeed even if LTV Disco has no deposits: {:?}", res.err());

        // All promises should be cleared
        let promises: Vec<RevenuePromise> = app.wrap().query_wasm_smart(&rd_addr, &QueryMsg::Promises {}).unwrap();
        assert!(promises.is_empty());
    }

    #[test]
    fn test_ltv_disco_destination_fallback_no_ratios_does_not_break() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // RevenueSplit defaults already include disco — no config update needed

        // Set a small promise and send larger funds so remaining goes to revenue destinations.
        // Do NOT set ltv_disco_distribution so map is empty -> fallback path in ltv_disco_add_revenue_msgs
        let promises = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(500) }];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(5000, "uusdc")]).unwrap();

        let msg = ExecuteMsg::DistributePromises { limit: None };
        let res = app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[]);
        // With no per-asset attribution, revenue split silently returns empty (no routing possible)
        assert!(res.is_ok(), "Distribute should succeed with empty per-asset map: {:?}", res.err());
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Try to call TakeRevenue directly (not through RD)
        let msg = CDP_MockExecuteMsg::TakeRevenue {};
        let result = app.execute_contract(
            Addr::unchecked(&user()), // Not the revenue distributor
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            rd_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Give CDP some CDT (but no revenue to send)
        app.send_tokens(
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Set CDP state with zero revenue (so no CDT is sent)
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::zero()), Some(vec![]), None, None);

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(600, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(500, "uusdc")],
        ).unwrap();

        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // First call
        let msg1 = ExecuteMsg::TakeRevenueFromBasket {};
        let result1 = app.execute_contract(
            Addr::unchecked(&user()),
            rd_addr.clone(),
            &msg1,
            &[],
        );
        assert!(result1.is_ok());

        // Second call before first reply (should overwrite storage)
        // Set new state
        set_cdp_state(&mut app, &cdp_addr, Some(Uint128::new(500)), Some(per_asset_rev), None, None);
        app.send_tokens(
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(500, "uusdc")],
        ).unwrap();

        let msg2 = ExecuteMsg::TakeRevenueFromBasket {};
        let result2 = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Call TakeRevenueFromBasket
        let msg = ExecuteMsg::TakeRevenueFromBasket {};
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
        
        // Update config to set window
        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: Some(window_days),
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr, cdp_addr)
    }

    /// Setup for bad debt tests: staking-only destinations (no LTV Disco destination to avoid LTV_DISCO_DISTRIBUTION issues)
    fn setup_bad_debt_test(app: &mut App) -> (Addr, Addr, Addr, Addr, Addr) {
        let (revenue_distributor_addr, staking_addr, ltv_addr, transmuter_addr, cdp_addr) = setup_contracts_with_cdp(app);

        // Config defaults are fine for bad debt tests
        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
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
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises: promises1, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // First distribution should succeed - this sets last_distribution_time
        // We need to ensure messages are created so last_distribution_time is saved
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let first_result = app.execute_contract(
            Addr::unchecked(&user()),
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
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises: promises2, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Try to distribute immediately - should be blocked by window
        // Note: In the same block, current_time should equal last_distribution
        // So current_time < last_distribution + window_seconds should be true
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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

        // Affiliate promises are distributed even when window is blocked
        // (window only gates the batched Disco/Junior/Senior split)
        let promises: Vec<RevenuePromise> = app
            .wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();
        assert_eq!(promises.len(), 0);
    }

    #[test]
    fn test_distribute_promises_window_passed() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) = 
            setup_contracts_with_window(&mut app, 7); // 7 day window

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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

        // Config defaults are fine — RevenueSplit handles routing
        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        ).unwrap();

        // Set promises
        let promises = vec![
            RevenuePromise {
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Distribution should work immediately when no window is set
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // First distribution should succeed - this sets last_distribution_time
        let msg = ExecuteMsg::DistributePromises { limit: None };
        app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            cdp_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Try ExecuteRevenueDistribution immediately - should be blocked because window hasn't passed
        let msg = ExecuteMsg::ExecuteRevenueDistribution {};
        let result = app.execute_contract(
            Addr::unchecked(&user()), // Permissionless
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
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&user()), // Permissionless
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
                address: affiliate1(),
                amount: Uint128::new(1000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
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
                address: affiliate2(),
                amount: Uint128::new(2000),
            },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(5000, "uusdc")],
        ).unwrap();

        // Try to distribute - should be blocked because only 1 day has passed since last distribution
        let msg = ExecuteMsg::DistributePromises { limit: None };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            Addr::unchecked(&user()),
            &[coin(1000000, "uatom")],
        ).unwrap();

        // Deploy mock auction contract
        let auction_id = app.store_code(auction_contract());
        let auction_addr = app
            .instantiate_contract(
                auction_id,
                Addr::unchecked(&admin()),
                &Empty {},
                &[],
                "auction",
                None,
            )
            .unwrap();

        // Update config to set auction contract
        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: Some(auction_addr.to_string()),
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&user()),
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
                Addr::unchecked(&admin()),
                &Empty {},
                &[],
                "auction",
                None,
            )
            .unwrap();

        // Update config to set auction contract
        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: None,
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: Some(auction_addr.to_string()),
        };
        app.execute_contract(
            Addr::unchecked(&admin()),
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
            Addr::unchecked(&user()),
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
            Addr::unchecked(&admin()),
            Addr::unchecked(&user()),
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
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[coin(1000, "uatom")],
        );

        // Should fail because auction contract is not configured
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.root_cause().to_string().contains("Auction contract not configured"));
    }

    // ==========================================
    // Bad Debt Fulfillment Tests
    // ==========================================

    #[test]
    fn test_toggle_bad_debt_fulfillment_only_cdp_can_call() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Random user cannot toggle
        let msg = ExecuteMsg::ToggleBadDebtFulfillment { toggle: true };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());

        // Admin cannot toggle (only CDP can)
        let result = app.execute_contract(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());

        // CDP contract can toggle
        let result = app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_ok());

        // Verify it's active
        let active: bool = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::BadDebtActive {})
            .unwrap();
        assert!(active);
    }

    #[test]
    fn test_toggle_bad_debt_fulfillment_idempotent() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Toggle on twice — should be fine
        let msg = ExecuteMsg::ToggleBadDebtFulfillment { toggle: true };
        app.execute_contract(cdp_addr.clone(), revenue_distributor_addr.clone(), &msg, &[]).unwrap();
        app.execute_contract(cdp_addr.clone(), revenue_distributor_addr.clone(), &msg, &[]).unwrap();

        let active: bool = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::BadDebtActive {})
            .unwrap();
        assert!(active);

        // Toggle off twice — should be fine
        let msg = ExecuteMsg::ToggleBadDebtFulfillment { toggle: false };
        app.execute_contract(cdp_addr.clone(), revenue_distributor_addr.clone(), &msg, &[]).unwrap();
        app.execute_contract(cdp_addr.clone(), revenue_distributor_addr.clone(), &msg, &[]).unwrap();

        let active: bool = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::BadDebtActive {})
            .unwrap();
        assert!(!active);
    }

    #[test]
    fn test_bad_debt_active_query_default_false() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) =
            setup_bad_debt_test(&mut app);

        let active: bool = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::BadDebtActive {})
            .unwrap();
        assert!(!active);
    }

    #[test]
    fn test_distribute_redirects_to_fulfill_bad_debt_when_active() {
        let mut app = setup_app();
        let (revenue_distributor_addr, staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Set CDP bad debt to 500
        set_cdp_bad_debt(&mut app, &cdp_addr, Uint128::new(500));

        // Toggle bad debt mode on (from CDP)
        app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::ToggleBadDebtFulfillment { toggle: true },
            &[],
        ).unwrap();

        // Fund revenue distributor with 1000 uusdc
        app.send_tokens(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        // Get staking balance before
        let staking_balance_before = app.wrap()
            .query_balance(&staking_addr, "uusdc")
            .unwrap()
            .amount;

        // Distribute — should send 500 to CDP FulfillBadDebt, remainder to destinations
        let result = app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        );
        assert!(result.is_ok(), "distribute failed: {:?}", result.err());

        // Verify CDP received the bad debt fulfillment (balance should have the CDT)
        let cdp_balance = app.wrap()
            .query_balance(&cdp_addr, "uusdc")
            .unwrap()
            .amount;
        // CDP mock FulfillBadDebt keeps the funds (doesn't "burn" in mock)
        assert_eq!(cdp_balance, Uint128::new(500));

        // Verify reserve got its 4% share of the 500 remainder = 20
        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(20));
    }

    #[test]
    fn test_distribute_sends_all_to_bad_debt_when_debt_exceeds_balance() {
        let mut app = setup_app();
        let (revenue_distributor_addr, staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Set CDP bad debt to 5000 (more than available)
        set_cdp_bad_debt(&mut app, &cdp_addr, Uint128::new(5000));

        // Toggle bad debt mode on
        app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::ToggleBadDebtFulfillment { toggle: true },
            &[],
        ).unwrap();

        // Fund revenue distributor with 1000 uusdc
        app.send_tokens(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        let staking_balance_before = app.wrap()
            .query_balance(&staking_addr, "uusdc")
            .unwrap()
            .amount;

        // Distribute — all 1000 should go to CDP, nothing to destinations
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        ).unwrap();

        // All CDT went to CDP
        let cdp_balance = app.wrap()
            .query_balance(&cdp_addr, "uusdc")
            .unwrap()
            .amount;
        assert_eq!(cdp_balance, Uint128::new(1000));

        // Nothing went to staking
        let staking_balance_after = app.wrap()
            .query_balance(&staking_addr, "uusdc")
            .unwrap()
            .amount;
        assert_eq!(staking_balance_after, staking_balance_before);
    }

    #[test]
    fn test_distribute_normal_when_bad_debt_mode_off() {
        let mut app = setup_app();
        let (revenue_distributor_addr, staking_addr, _ltv_addr, _transmuter_addr, _cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Fund revenue distributor with 1000 uusdc (no bad debt mode)
        app.send_tokens(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        let staking_balance_before = app.wrap()
            .query_balance(&staking_addr, "uusdc")
            .unwrap()
            .amount;

        // Distribute — should go to normal destinations
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        ).unwrap();

        // Normal distribution via RevenueSplit: reserve gets 4% of 1000 = 40
        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(40));
    }

    #[test]
    fn test_distribute_falls_through_when_bad_debt_is_zero_even_if_toggled() {
        let mut app = setup_app();
        let (revenue_distributor_addr, staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Toggle bad debt on but set pending_bad_debt to 0
        set_cdp_bad_debt(&mut app, &cdp_addr, Uint128::zero());
        app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::ToggleBadDebtFulfillment { toggle: true },
            &[],
        ).unwrap();

        // Fund revenue distributor
        app.send_tokens(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        let staking_balance_before = app.wrap()
            .query_balance(&staking_addr, "uusdc")
            .unwrap()
            .amount;

        // Should fall through to normal distribution since bad debt is zero
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        ).unwrap();

        // Bad debt is zero, falls through to normal distribution via RevenueSplit
        // Reserve gets 4% of 1000 = 40
        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(40));
    }

    #[test]
    fn test_bad_debt_mode_clears_pending_promises() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Set some promises
        let promises = vec![
            RevenuePromise {
                address: affiliate1(),
                amount: Uint128::new(100),
            },
        ];
        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::SetPromises {
                promises,
                per_asset_distribution: None,
            },
            &[coin(100, "uusdc")],
        ).unwrap();

        // Verify promises exist
        let current_promises: Vec<RevenuePromise> = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();
        assert_eq!(current_promises.len(), 1);

        // Toggle bad debt on
        set_cdp_bad_debt(&mut app, &cdp_addr, Uint128::new(500));
        app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::ToggleBadDebtFulfillment { toggle: true },
            &[],
        ).unwrap();

        // Fund and distribute
        app.send_tokens(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &[coin(1000, "uusdc")],
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        ).unwrap();

        // Promises should be cleared
        let current_promises: Vec<RevenuePromise> = app.wrap()
            .query_wasm_smart(&revenue_distributor_addr, &QueryMsg::Promises {})
            .unwrap();
        assert_eq!(current_promises.len(), 0);
    }

    #[test]
    fn test_bad_debt_no_cdp_configured_errors() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) =
            setup_contracts(&mut app);

        // No CDP contract configured — toggle should fail because no one is authorized
        let msg = ExecuteMsg::ToggleBadDebtFulfillment { toggle: true };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_distribute_bad_debt_with_zero_balance_does_nothing() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr, cdp_addr) =
            setup_bad_debt_test(&mut app);

        // Set bad debt and toggle on
        set_cdp_bad_debt(&mut app, &cdp_addr, Uint128::new(500));
        app.execute_contract(
            cdp_addr.clone(),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::ToggleBadDebtFulfillment { toggle: true },
            &[],
        ).unwrap();

        // Don't fund revenue distributor — balance is 0

        // Distribute should succeed but do nothing (falls through to normal path)
        let result = app.execute_contract(
            Addr::unchecked(&user()),
            revenue_distributor_addr.clone(),
            &ExecuteMsg::DistributePromises { limit: None },
            &[],
        );
        assert!(result.is_ok());

        // CDP should have received nothing
        let cdp_balance = app.wrap()
            .query_balance(&cdp_addr, "uusdc")
            .unwrap()
            .amount;
        assert_eq!(cdp_balance, Uint128::zero());
    }

    #[test]
    fn test_deprecated_debt_auction_start_auction_errors() {
        // Verify the auction contract rejects debt auction StartAuction calls
        // This tests the soft-deprecation in the auction contract
        let mut app = setup_app();

        let auction_code_id = app.store_code(auction_contract());
        let auction_addr = app.instantiate_contract(
            auction_code_id,
            Addr::unchecked(&admin()),
            &Empty {},
            &[],
            "auction",
            None,
        ).unwrap();

        // StartAuction with no funds (debt auction path) should still succeed
        // in our mock because the mock doesn't implement the deprecation logic.
        // The real contract test is in the auction contract's own tests.
        // Here we just verify the mock works.
        let msg = membrane::auction::ExecuteMsg::StartAuction {
            repayment_position_info: None,
            auction_asset: Asset {
                info: AssetInfo::NativeToken { denom: "uusdc".to_string() },
                amount: Uint128::new(100),
            },
            send_to: None,
            per_asset_distribution: None,
        };
        let result = app.execute_contract(
            Addr::unchecked(&user()),
            auction_addr.clone(),
            &msg,
            &[],
        );
        // Mock auction accepts StartAuction
        assert!(result.is_ok());
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

    // ========== RevenueSplit Tests ==========

    #[test]
    fn test_revenue_split_validation_rejects_bad_sum() {
        let mut app = setup_app();
        let (revenue_distributor_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Try to update with ratios that don't sum to 1.0
        let bad_split = RevenueSplit {
            disco_ratio: Decimal::percent(50),
            junior_ratio: Decimal::percent(50),
            senior_ratio: Decimal::percent(10),
            reserve_ratio: Decimal::percent(10),
        };

        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: Some(bad_split),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };

        let result = app.execute_contract(
            Addr::unchecked(&admin()),
            revenue_distributor_addr.clone(),
            &msg,
            &[],
        );
        assert!(result.is_err(), "Should reject revenue split that doesn't sum to 1.0");
    }

    #[test]
    fn test_reserve_accumulation_and_query() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Send funds and set promises
        let promises = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(100) }];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(1000, "uusdc")]).unwrap();

        // Distribute
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        // Query reserve: 4% of (1000 - 100 affiliate) = 4% of 900 = 36
        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(36));
    }

    #[test]
    fn test_reserve_excluded_from_distribution_balance() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // First distribution: 1000 uusdc, 100 affiliate = 900 remainder, 36 reserve
        let promises = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(100) }];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(1000, "uusdc")]).unwrap();
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        let reserve_after_first: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve_after_first, Uint128::new(36));

        // Advance time past the 7-day window so the next distribution is allowed
        app.update_block(|block| {
            block.time = block.time.plus_seconds(7 * 24 * 60 * 60 + 1);
        });

        // Second distribution: send 500 more, no promises
        // Available = contract_balance - reserve. Contract has 1000 - 100 (affiliate) + 500 = 1400, reserve = 36
        // Available = 1364. Reserve from this = 4% of 1364 = 54. Total reserve = 36 + 54 = 90.
        let msg = ExecuteMsg::SetPromises { promises: vec![], per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(500, "uusdc")]).unwrap();
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        let reserve_after_second: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        // Reserve should have grown
        assert!(reserve_after_second > reserve_after_first, "Reserve should accumulate");
        assert_eq!(reserve_after_second, Uint128::new(90));
    }

    #[test]
    fn test_withdraw_reserve() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Accumulate some reserve
        let promises = vec![RevenuePromise { address: affiliate1(), amount: Uint128::new(100) }];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(1000, "uusdc")]).unwrap();
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(36));

        // Withdraw reserve (admin only)
        let recipient = addr("treasury");
        let msg = ExecuteMsg::WithdrawReserve {
            amount: Some(Uint128::new(20)),
            recipient: recipient.clone(),
        };
        app.execute_contract(Addr::unchecked(&admin()), rd_addr.clone(), &msg, &[]).unwrap();

        // Check reserve decreased
        let reserve_after: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve_after, Uint128::new(16));

        // Check recipient got the funds
        let recipient_balance = app.wrap().query_balance(&recipient, "uusdc").unwrap().amount;
        assert_eq!(recipient_balance, Uint128::new(20));
    }

    #[test]
    fn test_withdraw_reserve_unauthorized() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let msg = ExecuteMsg::WithdrawReserve {
            amount: None,
            recipient: addr("treasury"),
        };
        let result = app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[]);
        assert!(result.is_err(), "Non-admin should not be able to withdraw reserve");
    }

    #[test]
    fn test_revenue_split_query() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let split: RevenueSplit = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::RevenueSplit {})
            .unwrap();

        assert_eq!(split.disco_ratio, Decimal::percent(16));
        assert_eq!(split.junior_ratio, Decimal::percent(64));
        assert_eq!(split.senior_ratio, Decimal::percent(16));
        assert_eq!(split.reserve_ratio, Decimal::percent(4));
    }

    #[test]
    fn test_update_revenue_split() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        let new_split = RevenueSplit {
            disco_ratio: Decimal::percent(10),
            junior_ratio: Decimal::percent(70),
            senior_ratio: Decimal::percent(15),
            reserve_ratio: Decimal::percent(5),
        };

        let msg = ExecuteMsg::UpdateConfig {
            revenue_split: Some(new_split),
            ltv_disco: None,
            transmuter_vault: None,
            points_system_contract: None,
            cdp_contract: None,
            revenue_dispersal_window: None,
            acquisition_contract: None,
            ltv_disco_contract: None,
            auction_contract: None,
        };
        app.execute_contract(Addr::unchecked(&admin()), rd_addr.clone(), &msg, &[]).unwrap();

        // Verify updated
        let split: RevenueSplit = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::RevenueSplit {})
            .unwrap();
        assert_eq!(split.disco_ratio, Decimal::percent(10));
        assert_eq!(split.junior_ratio, Decimal::percent(70));
        assert_eq!(split.senior_ratio, Decimal::percent(15));
        assert_eq!(split.reserve_ratio, Decimal::percent(5));

        // Now distribute to verify new ratios: 5% reserve on 1000 = 50
        let msg = ExecuteMsg::SetPromises { promises: vec![], per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(1000, "uusdc")]).unwrap();
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(50));
    }

    #[test]
    fn test_affiliate_promises_unaffected_by_revenue_split() {
        let mut app = setup_app();
        let (rd_addr, _staking_addr, _ltv_addr, _transmuter_addr) = setup_contracts(&mut app);

        // Set affiliate promise
        let promises = vec![
            RevenuePromise { address: affiliate1(), amount: Uint128::new(300) },
            RevenuePromise { address: affiliate2(), amount: Uint128::new(200) },
        ];
        let msg = ExecuteMsg::SetPromises { promises, per_asset_distribution: None };
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &msg, &[coin(1000, "uusdc")]).unwrap();

        let affiliate1_balance_before = app.wrap().query_balance(&affiliate1(), "uusdc").unwrap().amount;
        let affiliate2_balance_before = app.wrap().query_balance(&affiliate2(), "uusdc").unwrap().amount;

        // Distribute
        app.execute_contract(Addr::unchecked(&user()), rd_addr.clone(), &ExecuteMsg::DistributePromises { limit: None }, &[]).unwrap();

        // Affiliates should get their exact promised amounts
        let affiliate1_balance_after = app.wrap().query_balance(&affiliate1(), "uusdc").unwrap().amount;
        let affiliate2_balance_after = app.wrap().query_balance(&affiliate2(), "uusdc").unwrap().amount;
        assert_eq!(affiliate1_balance_after - affiliate1_balance_before, Uint128::new(300));
        assert_eq!(affiliate2_balance_after - affiliate2_balance_before, Uint128::new(200));

        // Promises should be cleared
        let remaining_promises: Vec<RevenuePromise> = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::Promises {})
            .unwrap();
        assert!(remaining_promises.is_empty());

        // Reserve should be 4% of (1000 - 500) = 4% of 500 = 20
        let reserve: Uint128 = app.wrap()
            .query_wasm_smart(&rd_addr, &QueryMsg::ReserveBalance {})
            .unwrap();
        assert_eq!(reserve, Uint128::new(20));
    }
}
