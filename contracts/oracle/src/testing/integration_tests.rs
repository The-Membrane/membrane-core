#[cfg(test)]
mod tests {

    use crate::helpers::OracleContract;

    use membrane::oracle::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::types::{AssetInfo, AssetOracleInfo, TWAPPoolInfo};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use osmosis_std::types::osmosis::twap::v1beta1::ArithmeticTwapToNowResponse;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Oracle Contract
    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(Response::default().into())
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked("contract3"),
                vec![coin(30_000_000_000_000, "mbrn_denom")],
            )
            .unwrap(); //contract3 = Builders contract
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(99, "error"), coin(101, "credit_fulldenom")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, OracleContract) {
        let mut app = mock_app();

        //Instaniate Osmosis Proxy
        let proxy_id = app.store_code(osmosis_proxy_contract());

        let osmosis_proxy_contract_addr = app
            .instantiate_contract(
                proxy_id,
                Addr::unchecked(ADMIN),
                &Osmo_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Oracle contract
        let oracle_id = app.store_code(oracle_contract());

        let msg = InstantiateMsg {
            owner: None,
            positions_contract: Some("cdp".to_string()),
        };

        let oracle_contract_addr = app
            .instantiate_contract(oracle_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let oracle_contract = OracleContract(oracle_contract_addr);

        (app, oracle_contract)
    }

    #[cfg(test)]
    mod oracle {

        use super::*;
        use membrane::oracle::{Config, AssetResponse, PriceResponse};

        #[test]
        fn add_edit() {
            let (mut app, oracle_contract) = proper_instantiate();

            //Unauthorized AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("axlusdc"),
                    }],
                    static_price: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful AddAsset for Basket 1
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("axlusdc"),
                    }],
                    static_price: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful AddAsset for Basket 2
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(2u128),
                    osmosis_pools_for_twap: vec![TWAPPoolInfo {
                        pool_id: 2u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("axlusdc"),
                    }],
                    static_price: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();


            //Successful EditAsset
            let msg = ExecuteMsg::EditAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: Some(AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![TWAPPoolInfo {
                        pool_id: 2u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    static_price: None,
                }),
                remove: false,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("cdp"), cosmos_msg).unwrap();

            //Assert Asset was edited
            let asset: Vec<AssetResponse> = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.addr(),
                    &QueryMsg::Assets {
                        asset_infos: vec![AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        }],
                    },
                )
                .unwrap();
            assert_eq!(asset[0].oracle_info[0].osmosis_pools_for_twap[0].pool_id, 2u64);

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("debit"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("debit"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    static_price: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
           

            //Successful Remove
            let msg = ExecuteMsg::EditAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: None,
                remove: true,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("cdp"), cosmos_msg).unwrap();

            //Assert Asset was removed
            app.wrap()
                .query_wasm_smart::<Vec<AssetResponse>>(
                    oracle_contract.addr(),
                    &QueryMsg::Assets {
                        asset_infos: vec![AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        }],
                    },
                )
                .unwrap_err();
        }

        #[test]
        fn queries() {
            let (mut app, oracle_contract) = proper_instantiate();

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![
                        TWAPPoolInfo {
                            pool_id: 1u64,
                            base_asset_denom: String::from("credit_fulldenom"),
                            quote_asset_denom: String::from("axlusdc"),
                        },
                        TWAPPoolInfo {
                            pool_id: 2u64,
                            base_asset_denom: String::from("axlusdc"),
                            quote_asset_denom: String::from("uosmo"),
                        },
                    ],
                    static_price: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query Price
            let price: PriceResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.addr(),
                    &QueryMsg::Price {
                        asset_info: AssetInfo::NativeToken {
                            denom: String::from("credit_fulldenom"),
                        },
                        twap_timeframe: 90u64,
                        basket_id: Some(Uint128::new(1u128)),
                    },
                )
                .unwrap();
            assert_eq!(price.price, Decimal::percent(100));

            //Successful AddAsset to a different basket
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("axlusdc"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(2u128),
                    osmosis_pools_for_twap: vec![],
                    static_price: Some(Decimal::one()),
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query static Price
            let price: PriceResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.addr(),
                    &QueryMsg::Price {
                        asset_info: AssetInfo::NativeToken {
                            denom: String::from("axlusdc"),
                        },
                        twap_timeframe: 90u64,
                        basket_id: Some(Uint128::new(2u128)),
                    },
                )
                .unwrap();
            assert_eq!(price.price, Decimal::one());
        }

        #[test]
        fn update_config() {
            let (mut app, oracle_contract) = proper_instantiate();

            //Successful AddAsset
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                positions_contract: Some(String::from("new_pos_contract")), 
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Liquidity
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked("new_owner"), 
                    positions_contract: Some(Addr::unchecked("new_pos_contract")), 
            });
        }
    }
}
