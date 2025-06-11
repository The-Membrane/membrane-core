//! CustomMockQuerier for simulating Osmosis modules (TWAP, poolmanager, tokenfactory) in unit tests.
// Place this in your test module and use `custom_mock_deps()` instead of `mock_dependencies()`.

use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage};
use cosmwasm_std::{to_binary, Binary, ContractResult, OwnedDeps, QuerierResult, QueryRequest, SystemResult};
use osmosis_std::types::osmosis::twap::v1beta1::GeometricTwapToNowResponse;
use osmosis_std::types::osmosis::poolmanager::v1beta1::EstimateSwapExactAmountOutResponse;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgMintResponse;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgBurnResponse;
use std::cell::RefCell;
use membrane::market_manager::Config as MarketManagerConfig;
use membrane::market_manager::QueryMsg as MMQueryMsg;
use cosmwasm_std::Addr;

/// CustomMockQuerier simulates Osmosis module queries for contract unit tests.
pub struct CustomMockQuerier {
    pub base: MockQuerier,
    pub collateral_twap: RefCell<cosmwasm_std::Decimal>,
    pub debt_twap: RefCell<cosmwasm_std::Decimal>,
    pub manager_fee: RefCell<Option<cosmwasm_std::Decimal>>,
    // You can add fields here to control mock responses per test
}

impl CustomMockQuerier {
    pub fn new(base: MockQuerier) -> Self {
        Self {
            base,
            collateral_twap: RefCell::new(cosmwasm_std::Decimal::from_ratio(123u128, 100u128)),
            debt_twap: RefCell::new(cosmwasm_std::Decimal::one()),
            manager_fee: RefCell::new(None),
        }
    }
    pub fn set_collateral_twap(&self, price: cosmwasm_std::Decimal) {
        *self.collateral_twap.borrow_mut() = price;
    }
    pub fn set_debt_twap(&self, price: cosmwasm_std::Decimal) {
        *self.debt_twap.borrow_mut() = price;
    }
    /// Helper to set both prices at once
    pub fn set_twap_prices(&self, collateral: cosmwasm_std::Decimal, debt: cosmwasm_std::Decimal) {
        self.set_collateral_twap(collateral);
        self.set_debt_twap(debt);
    }
    pub fn set_manager_fee(&self, fee: cosmwasm_std::Decimal) {
        *self.manager_fee.borrow_mut() = Some(fee);
    }
}

impl cosmwasm_std::Querier for CustomMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest<cosmwasm_std::Empty> = cosmwasm_std::from_slice(bin_request).unwrap();
        // Handle manager contract fee query
        if let QueryRequest::Wasm(cosmwasm_std::WasmQuery::Smart { contract_addr, msg, .. }) = &request {
            if contract_addr == "manager_contract" {
                if let Ok(MMQueryMsg::Config {}) = cosmwasm_std::from_binary(msg) {
                    let fee = self.manager_fee.borrow().unwrap_or(cosmwasm_std::Decimal::zero());
                    let resp = MarketManagerConfig {
                        owner: Addr::unchecked("owner"),
                        managed_market_code_id: 0,
                        manager_whitelist: vec![],
                        osmosis_proxy_contract: Addr::unchecked("proxy"),
                        managed_market_fee: fee,
                    };
                    return SystemResult::Ok(ContractResult::Ok(to_binary(&resp).unwrap()));
                }
            }
        }
        // Handle TWAP query
        if let QueryRequest::Stargate { path, data } = &request {
            if path == "/osmosis.twap.v1beta1.Query/GeometricTwapToNow" {
                // Simple string search for denom in data
                let data_str = String::from_utf8_lossy(data);
                let price = if data_str.contains("atom") {
                    self.collateral_twap.borrow().to_string()
                } else if data_str.contains("cdt") || data_str.contains("ucdt") {
                    self.debt_twap.borrow().to_string()
                } else {
                    cosmwasm_std::Decimal::one().to_string()
                };
                let resp = GeometricTwapToNowResponse {
                    geometric_twap: price,
                };
                println!("resp: {:?}", resp);
                return SystemResult::Ok(ContractResult::Ok(to_binary(&resp).unwrap()));
            }
            // Handle poolmanager swap estimate
            if path == "/osmosis.poolmanager.v1beta1.Query/EstimateSwapExactAmountOut" {
                let resp = EstimateSwapExactAmountOutResponse {
                    token_in_amount: "1000".to_string(),
                };
                return SystemResult::Ok(ContractResult::Ok(to_binary(&resp).unwrap()));
            }
            // Handle tokenfactory mint (stub)
            if path == "/osmosis.tokenfactory.v1beta1.Msg/Mint" {
                let resp = MsgMintResponse {};
                return SystemResult::Ok(ContractResult::Ok(to_binary(&resp).unwrap()));
            }
            // Handle tokenfactory burn (stub)
            if path == "/osmosis.tokenfactory.v1beta1.Msg/Burn" {
                let resp = MsgBurnResponse {};
                return SystemResult::Ok(ContractResult::Ok(to_binary(&resp).unwrap()));
            }
            // Add more custom query handlers here as needed
        }
        // Fallback to base
        self.base.raw_query(bin_request)
    }
}

/// Helper to create OwnedDeps with CustomMockQuerier for use in contract tests.
pub fn custom_mock_deps() -> OwnedDeps<MockStorage, MockApi, CustomMockQuerier> {
    let base = MockQuerier::new(&[]);
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: CustomMockQuerier::new(base),
        custom_query_type: std::marker::PhantomData,
    }
}

// Example usage in your tests:
//
// ```
// use crate::testing::mock_querier::custom_mock_deps;
//
// #[test]
// fn test_with_osmosis_mock() {
//     let mut deps = custom_mock_deps();
//     // ... rest of your test logic ...
// }
// ``` 
 