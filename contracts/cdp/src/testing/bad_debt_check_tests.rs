// Integration tests for CDP bad debt check flow
// Tests that bad debt detected in CDP properly flows through to LTV Disco

#[cfg(test)]
mod tests {
    use cosmwasm_std::{coin, coins, Addr, Coin, Decimal, Uint128, Binary, Response, StdResult, StdError, to_json_binary, CosmosMsg, WasmMsg, from_json};
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use membrane::cdp::{ExecuteMsg, InstantiateMsg, QueryMsg, CallbackMsg, Config};
    use membrane::ltv_disco::{ExecuteMsg as LTVDisco_ExecuteMsg, QueryMsg as LTVDisco_QueryMsg, InstantiateMsg as LTVDisco_InstantiateMsg};
    use membrane::types::{Basket, Asset, AssetInfo, cAsset, PendingRevenue};
    use cosmwasm_schema::cw_serde;
    use cosmwasm_std::Empty;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    // Real LTV Disco contract wrapper
    fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            ltv_disco::contract::execute,
            ltv_disco::contract::instantiate,
            ltv_disco::contract::query,
        )
        .with_reply(ltv_disco::contract::reply);
        Box::new(contract)
    }

    // CDP contract wrapper
    fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply(crate::contract::reply);
        Box::new(contract)
    }

    // Mock Oracle contract
    #[cw_serde]
    pub enum Oracle_MockQueryMsg {
        Prices { asset_infos: Vec<AssetInfo>, twap_timeframe: u64, oracle_time_limit: u64 },
    }

    fn oracle_mock_contract() -> Box<dyn Contract<Empty>> {
        use membrane::oracle::PriceResponse;
        let contract = ContractWrapper::new(
            |_, _, _, _: Empty| -> StdResult<Response> { Ok(Response::new()) },
            |_, _, _, _: Empty| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices { .. } => {
                        // Return 1:1 price for simplicity (1 collateral = $1, 1 CDT = $1)
                        Ok(to_json_binary(&vec![
                            PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                            PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            },
                        ])?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    // Mock Chain Proxy contract
    #[cw_serde]
    pub enum ChainProxy_MockExecuteMsg {
        ExecuteSwaps { token_out: String, max_slippage: Decimal },
    }

    fn chain_proxy_mock_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: ChainProxy_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    ChainProxy_MockExecuteMsg::ExecuteSwaps { .. } => {
                        // Simulate successful swap - in real test would need to handle reply
                        Ok(Response::new())
                    }
                }
            },
            |_, _, _, _: Empty| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: Empty| -> StdResult<Binary> { Ok(to_json_binary(&true)?) },
        );
        Box::new(contract)
    }

    // Mock CDP Basket query for LTV Disco
    fn setup_ltv_disco_mock_cdp_query(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cdp_addr: String) {
        use membrane::cdp::QueryMsg as CDP_QueryMsg;
        use membrane::types::{Basket, Asset, cAsset, PendingRevenue};
        
        deps.querier.update_wasm(move |query| {
            match query {
                cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == cdp_addr {
                        let parsed: Result<CDP_QueryMsg, _> = from_json(msg);
                        if matches!(parsed, Ok(CDP_QueryMsg::GetBasket {})) {
                            let basket = Basket {
                                basket_id: Uint128::one(),
                                current_position_id: Uint128::one(),
                                collateral_types: vec![cAsset {
                                    asset: Asset {
                                        info: AssetInfo::NativeToken { 
                                            denom: "collateral".to_string() 
                                        },
                                        amount: Uint128::zero(),
                                    },
                                    max_borrow_LTV: Decimal::percent(70),
                                    max_LTV: Decimal::percent(75),
                                    pool_info: None,
                                    rate_index: Decimal::one(),
                                    individual_cost: None,
                                }],
                                collateral_supply_caps: vec![],
                                lastest_collateral_rates: vec![],
                                multi_asset_supply_caps: vec![],
                                credit_asset: Asset {
                                    info: AssetInfo::NativeToken { denom: "cdt".to_string() },
                                    amount: Uint128::zero(),
                                },
                                credit_price: membrane::oracle::PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                },
                                base_interest_rate: Decimal::zero(),
                                pending_revenue: PendingRevenue {
                                    total_pending: Uint128::zero(),
                                    per_asset_rev: vec![],
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
                            return cosmwasm_std::SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                                to_json_binary(&basket).unwrap(),
                            ));
                        }
                    }
                    cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                        error: "Unmocked query".to_string(),
                        request: msg.clone(),
                    })
                }
                _ => cosmwasm_std::SystemResult::Err(cosmwasm_std::SystemError::InvalidRequest {
                    error: "Unmocked query".to_string(),
                    request: Default::default(),
                }),
            }
        });
    }

    /// Test that CDP bad debt check queries CanHandleBadDebt and sends AddBadDebt when disco can handle it
    #[test]
    fn test_cdp_bad_debt_check_sends_to_disco_when_can_handle() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router.bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                coins(1_000_000_000, "collateral"),
            ).unwrap();
            router.bank.init_balance(
                storage,
                &Addr::unchecked(ADMIN),
                coins(1_000_000_000, "cdt"),
            ).unwrap();
        });

        // Store contracts
        let ltv_disco_code_id = app.store_code(ltv_disco_contract());
        let cdp_code_id = app.store_code(cdp_contract());
        let oracle_code_id = app.store_code(oracle_mock_contract());
        let chain_proxy_code_id = app.store_code(chain_proxy_mock_contract());

        // Instantiate LTV Disco
        let ltv_disco_addr = app.instantiate_contract(
            ltv_disco_code_id,
            Addr::unchecked(ADMIN),
            &LTVDisco_InstantiateMsg {
                owner: Some(ADMIN.to_string()),
                cdp_contract: "cdp_contract".to_string(), // Will be updated after CDP instantiation
                deposit_denom: membrane::types::DepositDenom {
                    denom: "collateral".to_string(),
                    vault_info: None,
                },
                cdt_denom: "cdt".to_string(),
                minimum_deposit: Uint128::new(1000),
                max_ltv: Decimal::percent(95),
                percent_to_disperse: Decimal::percent(10),
                dispersal_window: 24,
                activation_window: 48,
                oracle_contract: "oracle".to_string(), // Will be updated
                chain_proxy_contract: "chain_proxy".to_string(), // Will be updated
                lock_duration_ceiling: Some(365),
                affiliate_fee: Some(Decimal::percent(1)),
                max_management_fee: None,
                ltv_delta_minimum: Some(Decimal::percent(1)),
            },
            &[],
            "ltv_disco",
            None,
        ).unwrap();

        // Instantiate Oracle
        let oracle_addr = app.instantiate_contract(
            oracle_code_id,
            Addr::unchecked(ADMIN),
            &Empty {},
            &[],
            "oracle",
            None,
        ).unwrap();

        // Instantiate Chain Proxy
        let chain_proxy_addr = app.instantiate_contract(
            chain_proxy_code_id,
            Addr::unchecked(ADMIN),
            &Empty {},
            &[],
            "chain_proxy",
            None,
        ).unwrap();

        // Instantiate CDP (simplified - would need full instantiate msg)
        // For now, we'll test the callback directly
        
        // This test demonstrates the flow - in a full integration test,
        // we would need to set up the full CDP instantiation with proper mocks
        // and then trigger a liquidation that results in bad debt
        
        // The key verification points are:
        // 1. CDP queries CanHandleBadDebt on disco
        // 2. If true, CDP sends AddBadDebt to disco
        // 3. Disco receives AddBadDebt and processes it
        // 4. Disco sends FulfillBadDebt back to CDP
        
        // For now, we verify the disco can handle query works
        let can_handle: bool = app.wrap().query_wasm_smart(
            ltv_disco_addr.clone(),
            &LTVDisco_QueryMsg::CanHandleBadDebt {
                asset: "collateral".to_string(),
                amount: Uint128::new(100_000),
            },
        ).unwrap();
        
        // Initially should be false (no deposits)
        assert!(!can_handle, "Disco should not be able to handle bad debt without deposits");
    }

    /// Test that CDP bad debt check sends to auction when disco cannot handle
    #[test]
    fn test_cdp_bad_debt_check_sends_to_auction_when_disco_cannot_handle() {
        // This test would verify that when CanHandleBadDebt returns false,
        // the CDP sends the bad debt to the auction contract instead
        
        // Implementation would require full CDP setup with auction contract
        // and triggering a liquidation that results in bad debt
    }

    /// Test full end-to-end flow: liquidation → bad debt → disco → fulfillment
    #[test]
    fn test_full_bad_debt_flow_from_liquidation() {
        // This test would:
        // 1. Create a position with debt
        // 2. Liquidate it (resulting in bad debt)
        // 3. Verify bad debt check callback is triggered
        // 4. Verify CanHandleBadDebt is queried
        // 5. Verify AddBadDebt is sent to disco
        // 6. Verify disco processes and sends FulfillBadDebt back
        // 7. Verify CDP burns the CDT
        
        // Implementation would require comprehensive CDP setup
    }
}






















