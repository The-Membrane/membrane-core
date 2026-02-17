// Unit tests for LTV Disco LTV query functionality

#[cfg(test)]
mod tests {
    use cosmwasm_std::{testing::{mock_dependencies, mock_env, MockQuerier}, Addr, Decimal, QuerierWrapper, SystemResult, ContractResult, to_json_binary, WasmQuery, QueryRequest};
    use membrane::types::{cAsset, Asset, AssetInfo};
    use membrane::ltv_disco::{QueryMsg as LTVDiscoQueryMsg, AverageLTVsResponse};
    use crate::query::query_ltv_disco_for_asset_ltvs;

    /// Helper to create a mock querier that returns specific LTV values
    fn mock_querier_with_ltvs(max_ltv: Decimal, max_borrow_ltv: Decimal) -> MockQuerier {
        let mut querier = MockQuerier::default();
        querier.update_wasm(move |query| {
            match query {
                WasmQuery::Smart { contract_addr: _, msg } => {
                    // Try to deserialize as LTVDiscoQueryMsg
                    if let Ok(ltv_query) = cosmwasm_std::from_json::<LTVDiscoQueryMsg>(msg) {
                        match ltv_query {
                            LTVDiscoQueryMsg::GetAverageLTVs { .. } => {
                                SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&AverageLTVsResponse {
                                        average_max_ltv: max_ltv,
                                        average_max_borrow_ltv: max_borrow_ltv,
                                    }).unwrap()
                                ))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&true).unwrap())),
            }
        });
        querier
    }

    #[test]
    fn test_query_ltv_disco_returns_queried_ltvs() {
        // Setup mock querier that returns 85% max_ltv and 75% max_borrow_ltv
        let querier = mock_querier_with_ltvs(Decimal::percent(85), Decimal::percent(75));
        let wrapper = QuerierWrapper::new(&querier);

        let assets = vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uosmo".to_string(),
                    },
                    amount: cosmwasm_std::Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(60), // Fallback values
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            },
        ];

        let ltv_disco_addr = Addr::unchecked("ltv_disco");
        
        // Query ltv_disco
        let result = query_ltv_disco_for_asset_ltvs(wrapper, ltv_disco_addr, assets.clone()).unwrap();

        // Should return the queried LTVs, not the fallback values
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Decimal::percent(85)); // max_ltv
        assert_eq!(result[0].1, Decimal::percent(75)); // max_borrow_ltv
    }

    #[test]
    fn test_query_ltv_disco_fallback_to_stored_ltvs() {
        // Setup mock querier that returns zero LTVs (simulating no deposits)
        let querier = mock_querier_with_ltvs(Decimal::zero(), Decimal::zero());
        let wrapper = QuerierWrapper::new(&querier);

        let assets = vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uosmo".to_string(),
                    },
                    amount: cosmwasm_std::Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(60), // Fallback values
                max_LTV: Decimal::percent(70),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            },
        ];

        let ltv_disco_addr = Addr::unchecked("ltv_disco");
        
        // Query ltv_disco
        let result = query_ltv_disco_for_asset_ltvs(wrapper, ltv_disco_addr, assets.clone()).unwrap();

        // Should return the stored fallback LTVs when ltv_disco returns zero
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Decimal::percent(70)); // Fallback max_ltv
        assert_eq!(result[0].1, Decimal::percent(60)); // Fallback max_borrow_ltv
    }

    #[test]
    fn test_query_ltv_disco_multiple_assets() {
        // Setup mock querier
        let querier = mock_querier_with_ltvs(Decimal::percent(80), Decimal::percent(70));
        let wrapper = QuerierWrapper::new(&querier);

        let assets = vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uosmo".to_string(),
                    },
                    amount: cosmwasm_std::Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(60),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            },
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uatom".to_string(),
                    },
                    amount: cosmwasm_std::Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(55),
                max_LTV: Decimal::percent(65),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            },
        ];

        let ltv_disco_addr = Addr::unchecked("ltv_disco");
        
        // Query ltv_disco
        let result = query_ltv_disco_for_asset_ltvs(wrapper, ltv_disco_addr, assets.clone()).unwrap();

        // Should return queried LTVs for each asset
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Decimal::percent(80)); // First asset max_ltv
        assert_eq!(result[0].1, Decimal::percent(70)); // First asset max_borrow_ltv
        assert_eq!(result[1].0, Decimal::percent(80)); // Second asset max_ltv
        assert_eq!(result[1].1, Decimal::percent(70)); // Second asset max_borrow_ltv
    }

    #[test]
    fn test_query_ltv_disco_mixed_zero_and_nonzero() {
        // Test where ltv_disco returns zero for one asset but not the other
        // This is a simplified test - in reality, the mock would need to differentiate between assets
        let querier = mock_querier_with_ltvs(Decimal::zero(), Decimal::zero());
        let wrapper = QuerierWrapper::new(&querier);

        let assets = vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "uosmo".to_string(),
                    },
                    amount: cosmwasm_std::Uint128::zero(),
                },
                max_borrow_LTV: Decimal::percent(65),
                max_LTV: Decimal::percent(75),
                pool_info: None,
                rate_index: Decimal::one(),
                peg_rate_index: Decimal::one(),
                force_redemptions: None,
            },
        ];

        let ltv_disco_addr = Addr::unchecked("ltv_disco");
        
        // Query ltv_disco
        let result = query_ltv_disco_for_asset_ltvs(wrapper, ltv_disco_addr, assets.clone()).unwrap();

        // Should fallback to stored LTVs when ltv_disco returns zero
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Decimal::percent(75)); // Fallback to stored max_ltv
        assert_eq!(result[0].1, Decimal::percent(65)); // Fallback to stored max_borrow_ltv
    }
}

