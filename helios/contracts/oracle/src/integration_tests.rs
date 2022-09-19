#[cfg(test)]
mod tests {
    
    use crate::helpers::{ OracleContract };
        
    
    use cw20::BalanceResponse;
    use membrane::oracle::{ InstantiateMsg, QueryMsg, ExecuteMsg };
    use membrane::osmosis_proxy::{ GetDenomResponse };
    use membrane::types::{AssetInfo, Asset, VestingPeriod, StakeDeposit, AssetOracleInfo, TWAPPoolInfo };

    
    use osmo_bindings::{ SpotPriceResponse, PoolStateResponse, ArithmeticTwapToNowResponse };
    use cosmwasm_std::{Addr, Coin, Empty, Uint128, Decimal, Response, StdResult, Binary, to_binary, coin, attr, StdError };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor, BankKeeper};
    use schemars::JsonSchema;
    use serde::{ Deserialize, Serialize };


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
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        },
        BurnTokens {
            denom: String,
            amount: Uint128,
            burn_from_address: String,
        },
        CreateDenom {
            subdenom: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {
        SpotPrice {
            asset: String,
        },
        PoolState {
            id: u64,
        },
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        ArithmeticTwapToNow {
            id: u64,
            quote_asset_denom: String,
            base_asset_denom: String,
            start_time: i64,
        },
    }

    pub fn osmosis_proxy_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens { 
                            denom, 
                            amount, 
                            mint_to_address
                     } => {
                        
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => {
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::CreateDenom { 
                        subdenom
                    } => {

                        Ok(Response::new().add_attributes(vec![
                            attr("basket_id", "1"),
                            attr("subdenom", "credit_fulldenom")]
                        ))
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::SpotPrice { 
                        asset,
                    } => 
                        Ok(
                            to_binary(&SpotPriceResponse {
                                price: Decimal::one(),
                            })?
                        ),
                    Osmo_MockQueryMsg::PoolState { id } => 
                    if id == 99u64 {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 100_000_000 , "base" ), coin( 100_000_000 , "quote" ) ],
                                shares: coin( 100_000_000, "lp_denom" ),
                            }

                            )?
                        )
                    } else {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 49_999 , "credit_fulldenom" ) ],
                                shares: coin( 0, "shares" ),
                            }

                            )?
                        )
                    },
                    Osmo_MockQueryMsg::GetDenom { 
                        creator_address, 
                        subdenom 
                    } => {
                        Ok(
                            to_binary(&GetDenomResponse {
                                denom: String::from( "credit_fulldenom" ),
                            })?
                        )
                    },
                    Osmo_MockQueryMsg::ArithmeticTwapToNow { 
                        id, 
                        quote_asset_denom, 
                        base_asset_denom, 
                        start_time 
                    } => {
                        if id == 2u64 {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(450),
                                })?
                            )

                        } else {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(50),
                                })?
                            )

                        }
                    }
                }},
        );
        Box::new(contract)
    }


    
    
    fn mock_app() -> App {
            AppBuilder::new().build(|router, _, storage| {
                                    
                let bank = BankKeeper::new();
                
                bank.init_balance(storage, &Addr::unchecked("contract3"), vec![coin(30_000_000_000_000, "mbrn_denom")])
                .unwrap(); //contract3 = Builders contract                
                bank.init_balance(storage, &Addr::unchecked("coin_God"), vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked(USER), vec![coin(99, "error"), coin(101, "credit_fulldenom")])
                .unwrap();
              

                router
                    .bank = bank;
                    
            })
        }

    fn proper_instantiate(  ) -> ( App, OracleContract ) {
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
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            positions_contract: Some( "cdp".to_string() ),
        };        

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let oracle_contract = OracleContract(oracle_contract_addr);


        ( app, oracle_contract )
    }
   

    #[cfg(test)]
    mod oracle {
        
        use super::*;
        use cosmwasm_std::{BlockInfo, Uint64, WasmMsg};
        use cw20::Cw20ReceiveMsg;
        use membrane::{ types::{UserInfo, RepayPosition}, oracle::{PriceResponse, AssetResponse} };
        
        
        #[test]
        fn add_edit() {
            let (mut app, oracle_contract ) = proper_instantiate( );
            
            //Unauthorized AddAsset
            let msg = ExecuteMsg::AddAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                oracle_info: AssetOracleInfo { 
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![ TWAPPoolInfo { 
                        pool_id: 1u64, 
                        base_asset_denom: String::from("credit_fulldenom"), 
                        quote_asset_denom: String::from("axlusdc") 
                    }],
                    static_price: None,
             } 
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                oracle_info: AssetOracleInfo { 
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![ TWAPPoolInfo { 
                        pool_id: 1u64, 
                        base_asset_denom: String::from("credit_fulldenom"), 
                        quote_asset_denom: String::from("axlusdc") 
                    }],
                    static_price: None,
             } 
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();           
            

            //Query Price
            let price: PriceResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Price { 
                        asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                        twap_timeframe: 90u64, 
                        basket_id: Some( Uint128::new(1u128) ),
                    },
                )
                .unwrap();
            assert_eq!( price.avg_price, Decimal::percent(50) );
            
            //Successful EditAsset
            let msg = ExecuteMsg::EditAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                oracle_info: Some( AssetOracleInfo { 
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![ TWAPPoolInfo { 
                        pool_id: 2u64, 
                        base_asset_denom: String::from("credit_fulldenom"), 
                        quote_asset_denom: String::from("uosmo") 
                    }],
                    static_price: None,
             }  ),
                remove: false,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("cdp"), cosmos_msg).unwrap();     

                       
            //Assert Asset was edited
            let asset: AssetResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Asset { 
                        asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } 
                    }
                )
                .unwrap();
            assert_eq!( asset.oracle_info[0].osmosis_pools_for_twap[0].pool_id, 2u64 );

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("debit") }, 
                oracle_info: AssetOracleInfo { 
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![ TWAPPoolInfo { 
                        pool_id: 1u64, 
                        base_asset_denom: String::from("debit"), 
                        quote_asset_denom: String::from("uosmo") 
                    }],
                    static_price: None,
             }  
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();   

            //Query Price(s)
            let price: Vec<PriceResponse> = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Prices { 
                        asset_infos: vec![ AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, AssetInfo::NativeToken { denom: String::from("debit") } ], 
                        twap_timeframe: 90u64, 
                    },
                )
                .unwrap();
            assert_eq!( price[0].avg_price, Decimal::percent(50) );
            assert_eq!( price[1].avg_price, Decimal::percent(50) );

            //Successful Remove
            let msg = ExecuteMsg::EditAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                oracle_info: None,
                remove: true,
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("cdp"), cosmos_msg).unwrap();     
           
            //Assert Asset was removed
            app
                .wrap()
                .query_wasm_smart::<AssetResponse>(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Assets { 
                        asset_infos: vec![ AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ],
                    }
                )
                .unwrap_err();
        }
        
        #[test]
        fn queries(){

            let (mut app, oracle_contract ) = proper_instantiate( );           

            //Successful AddAsset
            let msg = ExecuteMsg::AddAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                oracle_info: AssetOracleInfo { 
                    basket_id: Uint128::new(1u128),
                    osmosis_pools_for_twap: vec![ 
                        TWAPPoolInfo { 
                            pool_id: 1u64, 
                            base_asset_denom: String::from("credit_fulldenom"), 
                            quote_asset_denom: String::from("axlusdc") 
                        },
                        TWAPPoolInfo { 
                            pool_id: 2u64, 
                            base_asset_denom: String::from("axlusdc"), 
                            quote_asset_denom: String::from("uosmo") 
                        }
                        ],
                    static_price: None,
             } 
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();    

            //Query Price
            let price: PriceResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Price { 
                        asset_info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") }, 
                        twap_timeframe: 90u64, 
                        basket_id: Some( Uint128::new(1u128) ),
                    },
                )
                .unwrap();
            assert_eq!( price.avg_price, Decimal::percent(225) );

            //Successful AddAsset to a different basket
            let msg = ExecuteMsg::AddAsset { 
                asset_info: AssetInfo::NativeToken { denom: String::from("axlusdc") }, 
                oracle_info: AssetOracleInfo { 
                    basket_id: Uint128::new(2u128),
                    osmosis_pools_for_twap: vec![ ],
                    static_price: Some( Decimal::one() ),
             } 
            };
            let cosmos_msg = oracle_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();    

            //Query static Price
            let price: PriceResponse = app
                .wrap()
                .query_wasm_smart(
                    oracle_contract.clone().addr(),
                    &QueryMsg::Price { 
                        asset_info: AssetInfo::NativeToken { denom: String::from("axlusdc") }, 
                        twap_timeframe: 90u64, 
                        basket_id: Some( Uint128::new(2u128) ),
                    },
                )
                .unwrap();
            assert_eq!( price.avg_price, Decimal::one() );

        }

    }
}