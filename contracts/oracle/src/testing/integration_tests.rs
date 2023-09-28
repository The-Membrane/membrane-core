#[cfg(test)]
mod tests {

    use crate::helpers::OracleContract;

    use membrane::oracle::{ExecuteMsg, InstantiateMsg, QueryMsg, PriceResponse};
    use membrane::osmosis_proxy::Config as OP_Config;
    use membrane::types::{AssetInfo, AssetOracleInfo, TWAPPoolInfo, PriceInfo, Asset, Basket, SupplyCap, Owner};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
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
    pub enum Osmo_MockQueryMsg {
        Config {},
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::Config {} => Ok(to_binary(&OP_Config {
                        owners: vec![
                            Owner { 
                                owner: Addr::unchecked("contract1"), 
                                total_minted: Uint128::zero(),
                                stability_pool_ratio: None, 
                                non_token_contract_auth: false,
                                is_position_contract: true
                            }
                        ],
                        liquidity_multiplier: None,
                        debt_auction: None,
                        positions_contract: None,
                        liquidity_contract: None,
                        oracle_contract: None,
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Positions Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasket {},
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket {} => Ok(to_binary(&Basket {
                        basket_id: Uint128::zero(),
                        current_position_id: Uint128::zero(),
                        collateral_types: vec![],
                        collateral_supply_caps: vec![
                            SupplyCap { 
                                asset_info: AssetInfo::NativeToken { denom: String::from("removable") }, 
                                current_supply: Uint128::zero(), 
                                debt_total: Uint128::zero(),  
                                supply_cap_ratio: Decimal::zero(), 
                                lp: false,
                                stability_pool_ratio_for_debt_cap: None,
                            },
                        ],
                        lastest_collateral_rates: vec![],
                        credit_asset: Asset { info: AssetInfo::NativeToken { denom: String::from("factory/cdt/#1") }, amount: Uint128::zero() },
                        credit_price: PriceResponse {
                            prices: vec![],
                            price: Decimal::zero(),
                            decimals: 6,
                        },
                        liq_queue: None,
                        base_interest_rate: Decimal::zero(),
                        pending_revenue: Uint128::zero(),
                        negative_rates: false,
                        cpc_margin_of_error: Decimal::zero(),
                        multi_asset_supply_caps: vec![],
                        frozen: false,
                        rev_to_stakers: true,
                        credit_last_accrued: 0,
                        rates_last_accrued: 0,
                        oracle_set: true,
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

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

    fn proper_instantiate() -> (App, OracleContract, Addr) {
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

        //Instaniate Positions contract
        let proxy_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                proxy_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Oracle contract
        let oracle_id = app.store_code(oracle_contract());

        let msg = InstantiateMsg {
            owner: None,
            positions_contract: Some(cdp_contract_addr.to_string()),
            osmosis_proxy_contract: Some(osmosis_proxy_contract_addr.to_string()),
        };

        let oracle_contract_addr = app
            .instantiate_contract(oracle_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let oracle_contract = OracleContract(oracle_contract_addr);

        (app, oracle_contract, cdp_contract_addr)
    }

    #[cfg(test)]
    mod oracle {

        use core::panic;
        use std::str::FromStr;

        use super::*;
        use membrane::oracle::{Config, AssetResponse};
        use membrane::math::{decimal_division, decimal_multiplication};
        use pyth_sdk_cw::PriceIdentifier;

        #[test]
        fn add_edit() {
            let (mut app, oracle_contract, cdp_contract) = proper_instantiate();

            //Unauthorized AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("credit_fulldenom"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    pools_for_osmo_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    is_usd_par: false,
                    lp_pool_info: None,
                    decimals: 6,
                    pyth_price_feed_id: None,
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
                    pools_for_osmo_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    is_usd_par: false,
                    lp_pool_info: None,
                    decimals: 6,
                    pyth_price_feed_id: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Successful AddAsset for Basket 2
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("removable"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(2u128),
                    pools_for_osmo_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("removable"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    is_usd_par: false,
                    lp_pool_info: None,
                    decimals: 6,
                    pyth_price_feed_id: None,
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
                    pools_for_osmo_twap: vec![TWAPPoolInfo {
                        pool_id: 2u64,
                        base_asset_denom: String::from("credit_fulldenom"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    is_usd_par: false,
                    lp_pool_info: None,
                    decimals: 6,
                    pyth_price_feed_id: None,
                }),
                remove: false,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(cdp_contract.clone(), cosmos_msg).unwrap();

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
            assert_eq!(asset[0].oracle_info[0].pools_for_osmo_twap[0].pool_id, 2u64);

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("debit"),
                },
                oracle_info: AssetOracleInfo {
                    basket_id: Uint128::new(1u128),
                    pools_for_osmo_twap: vec![TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: String::from("debit"),
                        quote_asset_denom: String::from("uosmo"),
                    }],
                    is_usd_par: false,
                    lp_pool_info: None,
                    decimals: 6,
                    pyth_price_feed_id: None,
                },
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
           

            //Successful Remove
            let msg = ExecuteMsg::EditAsset {
                asset_info: AssetInfo::NativeToken {
                    denom: String::from("removable"),
                },
                oracle_info: None,
                remove: true,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(cdp_contract, cosmos_msg).unwrap();

            //Assert Asset was removed
            app.wrap()
                .query_wasm_smart::<Vec<AssetResponse>>(
                    oracle_contract.addr(),
                    &QueryMsg::Assets {
                        asset_infos: vec![AssetInfo::NativeToken {
                            denom: String::from("removable"),
                        }],
                    },
                )
                .unwrap_err();
        }

        // #[test]
        // fn queries() {
        //     let (mut app, oracle_contract) = proper_instantiate();

        //     //Successful AddAsset
        //     let msg = ExecuteMsg::AddAsset {
        //         asset_info: AssetInfo::NativeToken {
        //             denom: String::from("credit_fulldenom"),
        //         },
        //         oracle_info: AssetOracleInfo {
        //             basket_id: Uint128::new(1u128),
        //             osmosis_pools_for_twap: vec![
        //                 TWAPPoolInfo {
        //                     pool_id: 1u64,
        //                     base_asset_denom: String::from("credit_fulldenom"),
        //                     quote_asset_denom: String::from("axlusdc"),
        //                 },
        //                 TWAPPoolInfo {
        //                     pool_id: 2u64,
        //                     base_asset_denom: String::from("axlusdc"),
        //                     quote_asset_denom: String::from("uosmo"),
        //                 },
        //             ],
        //             static_price: None,
        //         },
        //     };
        //     let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        //     //Query Price: No TWAP found
        //     let err = app
        //         .wrap()
        //         .query_wasm_smart::<PriceResponse>(
        //             oracle_contract.addr(),
        //             &QueryMsg::Price {
        //                 asset_info: AssetInfo::NativeToken {
        //                     denom: String::from("credit_fulldenom"),
        //                 },
        //                 twap_timeframe: 90u64,
        //                 basket_id: Some(Uint128::new(1u128)),
        //             },
        //         )
        //         .unwrap_err();

        //     //Successful AddAsset to a different basket
        //     let msg = ExecuteMsg::AddAsset {
        //         asset_info: AssetInfo::NativeToken {
        //             denom: String::from("axlusdc"),
        //         },
        //         oracle_info: AssetOracleInfo {
        //             basket_id: Uint128::new(2u128),
        //             osmosis_pools_for_twap: vec![],
        //             static_price: Some(Decimal::one()),
        //         },
        //     };
        //     let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
        //     app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        //     //Query static Price
        //     let price: PriceResponse = app
        //         .wrap()
        //         .query_wasm_smart(
        //             oracle_contract.addr(),
        //             &QueryMsg::Price {
        //                 asset_info: AssetInfo::NativeToken {
        //                     denom: String::from("axlusdc"),
        //                 },
        //                 twap_timeframe: 90u64,
        //                 basket_id: Some(Uint128::new(2u128)),
        //             },
        //         )
        //         .unwrap();
        //     assert_eq!(price.price, Decimal::one());
        // }

        #[test]
        fn median_test() {
            //Create a vector of PriceInfo
            let mut prices: Vec<PriceInfo> = vec![];
            prices.push(PriceInfo {
                price: Decimal::from_ratio(3u128, 1u128),
                source: String::from("osmosis"),
            });

            prices.sort_by(|a, b| a.price.cmp(&b.price));
                        
            //Get Median price
            let median_price = if prices.len() % 2 == 0 {
                let median_index = prices.len() / 2;

                //Add the two middle prices and divide by 2
                decimal_division(prices[median_index].price + prices[median_index-1].price, Decimal::percent(2_00)).unwrap()
                
            } else if prices.len() != 1 {
                let median_index = prices.len() / 2;
                prices[median_index].price
            } else {
                prices[0].price
            };

            assert_eq!(median_price, Decimal::from_ratio(3u128, 1u128));

            //Add another price
            prices.push(PriceInfo {
                price: Decimal::from_ratio(2u128, 1u128),
                source: String::from("osmosis"),
            });

            prices.sort_by(|a, b| a.price.cmp(&b.price));
                        
            //Get Median price
            let median_price = if prices.len() % 2 == 0 {
                let median_index = prices.len() / 2;

                //Add the two middle prices and divide by 2
                decimal_division(prices[median_index].price + prices[median_index-1].price, Decimal::percent(2_00)).unwrap()
                
            } else if prices.len() != 1 {
                let median_index = prices.len() / 2;
                prices[median_index].price
            } else {
                prices[0].price
            };

            //Median should be 2.5 from 2 and 3
            assert_eq!(median_price, Decimal::from_ratio(5u128, 2u128));

            //Add another price
            prices.push(PriceInfo {
                price: Decimal::from_ratio(1u128, 2u128),
                source: String::from("osmosis"),
            });

            prices.sort_by(|a, b| a.price.cmp(&b.price));
                        
            //Get Median price
            let median_price = if prices.len() % 2 == 0 {
                let median_index = prices.len() / 2;

                //Add the two middle prices and divide by 2
                decimal_division(prices[median_index].price + prices[median_index-1].price, Decimal::percent(2_00)).unwrap()
                
            } else if prices.len() != 1 {
                let median_index = prices.len() / 2;
                prices[median_index].price
            } else {
                prices[0].price
            };

            assert_eq!(median_price, Decimal::from_ratio(2u128, 1u128));
        }

        #[test]
        fn scaling_test() {
            let amount = Decimal::from_ratio(Uint128::new(187931653491861157), Uint128::new(1));
            let price = decimal_multiplication(Decimal::from_str("0.000000000000001954").unwrap(), amount).unwrap();
            panic!("{}", price);
            let quote_price;
            let price = 78574968;
            let expo: i32 = -6;
            //Scale price using given exponent
            match expo > 0 {
                true => {
                    quote_price = decimal_multiplication(
                        Decimal::from_str(&price.to_string()).unwrap(), 
                        Decimal::from_ratio(Uint128::new(10), Uint128::one()).checked_pow(expo as u32).unwrap()
                    ).unwrap();
                },
                //If the exponent is negative we divide, it should be for most if not all
                false => {
                    quote_price = decimal_division(
                        Decimal::from_str(&price.to_string()).unwrap(), 
                        Decimal::from_ratio(Uint128::new(10), Uint128::one()).checked_pow((expo*-1) as u32).unwrap()
                    ).unwrap();
                }
            };
            panic!("{}", quote_price);
        }

        #[test]
        fn update_config() {
            let (mut app, oracle_contract, cdp_contract) = proper_instantiate();

            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                positions_contract: Some(String::from("new_pos_contract")), 
                osmosis_proxy_contract: Some(String::from("new_osmosis_proxy_contract")),
                pyth_osmosis_address: Some(String::from("new_pyth_osmosis_address")),
                osmo_usd_pyth_feed_id: Some(PriceIdentifier::from_hex("63f341689d98a12ef60a5cff1d7f85c70a9e17bf1575f0e7c0b2512d48b1c8b3").unwrap()),
                pools_for_usd_par_twap: None,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Config
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
                    owner: Addr::unchecked(ADMIN), 
                    positions_contract: Some(Addr::unchecked("new_pos_contract")), 
                    osmosis_proxy_contract: Some(Addr::unchecked("new_osmosis_proxy_contract")),
                    pyth_osmosis_address: Some(Addr::unchecked("new_pyth_osmosis_address")),
                    osmo_usd_pyth_feed_id: PriceIdentifier::from_hex("63f341689d98a12ef60a5cff1d7f85c70a9e17bf1575f0e7c0b2512d48b1c8b3").unwrap(),
                    pools_for_usd_par_twap: vec![],
            });

            //Successful ownership transfer
            let msg = ExecuteMsg::UpdateConfig { 
                owner: None,
                positions_contract: None,
                osmosis_proxy_contract: None,
                pyth_osmosis_address: None,
                osmo_usd_pyth_feed_id: None,
                pools_for_usd_par_twap: None,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();

            
            //Query Config
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
                    osmosis_proxy_contract: Some(Addr::unchecked("new_osmosis_proxy_contract")),
                    pyth_osmosis_address: Some(Addr::unchecked("new_pyth_osmosis_address")),
                    osmo_usd_pyth_feed_id: PriceIdentifier::from_hex("63f341689d98a12ef60a5cff1d7f85c70a9e17bf1575f0e7c0b2512d48b1c8b3").unwrap(),
                    pools_for_usd_par_twap: vec![],
            });
        }
    }
}
