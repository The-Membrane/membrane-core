#[cfg(test)]
mod tests {
    
    use crate::helpers::{ DebtContract };
       
    
    use cw20::BalanceResponse;
    use membrane::debt_auction::{ InstantiateMsg, QueryMsg, ExecuteMsg };
    use membrane::builder_vesting::{ QueryMsg as BVQueryMsg, AllocationResponse };
    use membrane::oracle::{PriceResponse, AssetResponse};
    use membrane::positions::BasketResponse;
    use membrane::staking::{ RewardsResponse, StakedResponse, ConfigResponse as StakingConfigResponse };
    use membrane::osmosis_proxy::{ GetDenomResponse };
    use membrane::math::Uint256;
    use membrane::types::{AssetInfo, Asset, VestingPeriod, StakeDeposit, AssetOracleInfo, TWAPPoolInfo };

    
    use osmo_bindings::{ SpotPriceResponse, PoolStateResponse, ArithmeticTwapToNowResponse };
    use cosmwasm_std::{Addr, Coin, Empty, Uint128, Decimal, Response, StdResult, Binary, to_binary, coin, attr, StdError };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor, BankKeeper};
    use schemars::JsonSchema;
    use serde::{ Deserialize, Serialize };


    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Debt Contract
    pub fn debt_contract() -> Box<dyn Contract<Empty>> {
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
                        if amount != Uint128::new(105_319u128) && amount != Uint128::new(1_063u128) {
                            panic!("{}", amount)
                        }
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
                        if base_asset_denom == String::from("base") {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        } else {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        }
                    }
                }},
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockExecuteMsg {
        AddAsset {
            asset_info: AssetInfo,
            oracle_info: AssetOracleInfo,
        }
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Oracle_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            basket_id: Option<Uint128>,
        },
        Asset {
            asset_info: AssetInfo,
        },
    }

    pub fn oracle_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Oracle_MockExecuteMsg::AddAsset { 
                        asset_info, 
                        oracle_info, 
                    }  => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Price { 
                        asset_info,
                        twap_timeframe,
                        basket_id 
                    } => {

                        Ok( to_binary(&PriceResponse { 
                            prices: vec![],
                            avg_price: Decimal::one(),
                        })? )
                        
                        
                    },
                    Oracle_MockQueryMsg::Asset { asset_info } => {
                        Ok( to_binary(&AssetResponse { 
                            asset_info: AssetInfo::NativeToken { denom: String::from("denom") },
                            oracle_info: vec![ AssetOracleInfo {
                                basket_id: Uint128::new(1u128),
                                osmosis_pools_for_twap: vec![ TWAPPoolInfo {
                                    pool_id: 0u64,
                                    base_asset_denom: String::from("denom"),
                                    quote_asset_denom: String::from("denom"),
                                }],
                                static_price: None,
                            }],
                        })? )
                    },
                }  },
        );
        Box::new(contract)
    }

    //Mock CDP Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {
        Repay {
            basket_id: Uint128,
            position_id: Uint128,
            position_owner: Option<String>,
        },
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasket { basket_id: Uint128 },
    }

    pub fn cdp_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::Repay { 
                        basket_id,
                        position_id,
                        position_owner,
                    }  => {                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    CDP_MockQueryMsg::GetBasket {
                        basket_id 
                    } => {

                        Ok( to_binary(&BasketResponse {
                            owner: String::from("owner"),
                            basket_id: String::from(""),
                            current_position_id: String::from(""),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            credit_asset: Asset { info: AssetInfo::NativeToken{ denom: String::from("") }, amount: Uint128::zero() },
                            credit_price: Decimal::one(),
                            liq_queue: String::from(""),
                            base_interest_rate: Decimal::zero(),
                            liquidity_multiplier: Decimal::zero(),
                            desired_debt_cap_util: Decimal::zero(),
                            pending_revenue: Uint128::zero(),
                            negative_rates: true,
                        })? )
                                               
                    }
                }  },
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
                bank.init_balance(storage, &Addr::unchecked(USER), vec![coin(99, "error"), coin(101_000, "credit_fulldenom")])
                .unwrap();
              

                router
                    .bank = bank;
                    
            })
        }

    fn proper_instantiate(  ) -> ( App, DebtContract, Addr ) {
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
       
        //Instaniate Oracle Contract
        let oracle_id = app.store_code(oracle_contract());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate CDP Contract
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        

        //Instantiate Gov contract
        let debt_id = app.store_code(debt_contract());

        let msg = InstantiateMsg {
            owner: None,
            oracle_contract: oracle_contract_addr.to_string(),
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            positions_contract: cdp_contract_addr.to_string(),
            twap_timeframe: 90u64,
            mbrn_denom: String::from("mbrn_denom"),
            initial_discount: Decimal::percent(1),
            discount_increase_timeframe: 60u64,
            discount_increase: Decimal::percent(1),
        };        

        let debt_contract_addr = app
            .instantiate_contract(
                debt_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let debt_contract = DebtContract(debt_contract_addr);


        ( app, debt_contract, cdp_contract_addr )
    }
   


    mod debt_auction {
        
        use super::*;
        use cosmwasm_std::{BlockInfo, Uint64, WasmMsg};
        use cw20::Cw20ReceiveMsg;
        use membrane::{ types::{UserInfo, RepayPosition}, debt_auction::AuctionResponse };
        
        
        #[test]
        fn start_auction() {
            let (mut app, debt_contract, cdp_contract ) = proper_instantiate( );
            
            //Unauthorized StartAuction
            let msg = ExecuteMsg::StartAuction { 
                repayment_position_info: UserInfo { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: String::from("owner") 
                }, 
                debt_asset: Asset {
                    info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
                    amount: Uint128::new(100u128), 
                },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

            //Successful StartAuction
            let msg = ExecuteMsg::StartAuction { 
                repayment_position_info: UserInfo { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: String::from("owner") 
                }, 
                debt_asset: Asset {
                    info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
                    amount: Uint128::new(100u128), 
                },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();           

            //Assert Auction Response
            let auction: Vec<AuctionResponse> = app
                .wrap()
                .query_wasm_smart(
                    debt_contract.clone().addr(),
                    &QueryMsg::OngoingAuctions { 
                        debt_asset: Some(AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                        limit: None,
                        start_without: None,
                    },
                )
                .unwrap();
                assert_eq!( auction[0].auction_start_time, 1571797419u64);
                assert_eq!( auction[0].basket_id_price_source, Uint128::new(1u128));
                assert_eq!( auction[0].remaining_recapitalization,  Uint128::new(100u128));
                assert_eq!( auction[0].repayment_positions, vec![
                    RepayPosition { 
                        repayment: Uint128::new(100u128), 
                        position_info: UserInfo { 
                            basket_id: Uint128::new(1u128), 
                            position_id: Uint128::new(1u128), 
                            position_owner: String::from("owner"),
                        } }
                ] );

            //Successful Start adding to existing auction
            let msg = ExecuteMsg::StartAuction { 
                repayment_position_info: UserInfo { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: String::from("owner") 
                }, 
                debt_asset: Asset {
                    info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
                    amount: Uint128::new(100u128), 
                },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id } );
            app.execute( cdp_contract, cosmos_msg ).unwrap();

            //Assert Auction Response
            let auction: Vec<AuctionResponse> = app
            .wrap()
            .query_wasm_smart(
                debt_contract.clone().addr(),
                &QueryMsg::OngoingAuctions { 
                    debt_asset: Some(AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                    limit: None,
                    start_without: None,
                },
            )
            .unwrap();
            assert_eq!( auction[0].auction_start_time, 1571797419u64); //Start_time doesn't change
            assert_eq!( auction[0].basket_id_price_source, Uint128::new(1u128));
            assert_eq!( auction[0].remaining_recapitalization,  Uint128::new(200u128));
            assert_eq!( auction[0].repayment_positions, vec![
                RepayPosition { 
                    repayment: Uint128::new(100u128), 
                    position_info: UserInfo { 
                        basket_id: Uint128::new(1u128), 
                        position_id: Uint128::new(1u128), 
                        position_owner: String::from("owner"),
                    } }, 
                RepayPosition { 
                    repayment: Uint128::new(100u128), 
                    position_info: UserInfo { 
                        basket_id: Uint128::new(1u128), 
                        position_id: Uint128::new(1u128), 
                        position_owner: String::from("owner"),
                    } }
            ] );
           
        }


        #[test]
        fn swap_For_mbrn() {
            let (mut app, debt_contract, cdp_contract ) = proper_instantiate( );
            

            //Successful StartAuction
            let msg = ExecuteMsg::StartAuction { 
                repayment_position_info: UserInfo { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: String::from("owner") 
                }, 
                debt_asset: Asset {
                    info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
                    amount: Uint128::new(100_000u128), 
                },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();          
        
            //Errored Swap, invalid asset
            let msg = ExecuteMsg::SwapForMBRN {  };
            let cosmos_msg = debt_contract.call(msg, vec![ coin(99, "error") ]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();    
            assert_eq!( err.root_cause().to_string(), String::from("Invalid Asset: error") );

            //Successful Partial Swap
            let msg = ExecuteMsg::SwapForMBRN {  };
            let cosmos_msg = debt_contract.call(msg, vec![ coin(99_000, "credit_fulldenom") ]).unwrap();
            app.set_block( BlockInfo { 
                height: app.block_info().height, 
                time: app.block_info().time.plus_seconds(300u64), //60 * 5
                chain_id: app.block_info().chain_id } );
            let res = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();                       

            ///Mint amount asserted in the contract definition

            //Assert Auction partial fulfillment
            let auction: Vec<AuctionResponse> = app
            .wrap()
            .query_wasm_smart(
                debt_contract.clone().addr(),
                &QueryMsg::OngoingAuctions { 
                    debt_asset: Some(AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                    limit: None,
                    start_without: None,
                },
            )
            .unwrap();

            assert_eq!( auction[0].auction_start_time, 1571797419u64); 
            assert_eq!( auction[0].basket_id_price_source, Uint128::new(1u128));
            assert_eq!( auction[0].remaining_recapitalization,  Uint128::new(1_000u128));
            assert_eq!( auction[0].repayment_positions, vec![
                RepayPosition { 
                    repayment: Uint128::new(1_000u128), 
                    position_info: UserInfo { 
                        basket_id: Uint128::new(1u128), 
                        position_id: Uint128::new(1u128), 
                        position_owner: String::from("owner"),
                    } }
            ] );
            
            //Successful Overpay Swap
            let msg = ExecuteMsg::SwapForMBRN {  };
            let cosmos_msg = debt_contract.call(msg, vec![ coin( 2_000, "credit_fulldenom" ) ]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();    
            
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 1_000, "credit_fulldenom" ), coin( 99, "error" ) ]);

            //Assert Auction is empty
            let err = app
            .wrap()
            .query_wasm_smart::<Vec<AuctionResponse>>(
                debt_contract.clone().addr(),
                &QueryMsg::OngoingAuctions { 
                    debt_asset: Some( AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                    limit: None,
                    start_without: None,
                },
            )
            .unwrap_err();
            assert_eq!(err.to_string(), String::from("Generic error: Querier contract error: Generic error: Auction recapitalization amount empty"));


            //Assert Asset is still valid
            let valid_assets: Vec<AssetInfo> = app
            .wrap()
            .query_wasm_smart(
                debt_contract.clone().addr(),
                &QueryMsg::ValidDebtAssets { 
                    debt_asset: Some( AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                    limit: None,
                    start_without: None,
                },
            )
            .unwrap();
            assert_eq!(valid_assets, vec![ AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ]);

        }

        #[test]
        fn remove_auction() {
            let (mut app, debt_contract, cdp_contract ) = proper_instantiate( );
            

            //Successful StartAuction
            let msg = ExecuteMsg::StartAuction { 
                repayment_position_info: UserInfo { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: String::from("owner") 
                }, 
                debt_asset: Asset {
                    info: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
                    amount: Uint128::new(100u128), 
                },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap(); 
            
            //Successful RemoveAuction: InvalidAsset
            let msg = ExecuteMsg::RemoveAuction { 
                debt_asset: AssetInfo::NativeToken { denom: String::from("invalid_asset") }
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //assert_eq!( err.root_cause().to_string(), String::from("Invalid Asset: invalid_asset") );
            
            //Successful RemoveAuction
            let msg = ExecuteMsg::RemoveAuction { 
                debt_asset: AssetInfo::NativeToken { denom: String::from("credit_fulldenom") },
            };
            let cosmos_msg = debt_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();  
            

            //Assert Auction removal
            app
                .wrap()
                .query_wasm_smart::< Vec<AuctionResponse>>(
                    debt_contract.clone().addr(),
                    &QueryMsg::OngoingAuctions { 
                        debt_asset: Some(AssetInfo::NativeToken { denom: String::from("credit_fulldenom") } ),
                        limit: None,
                        start_without: None,
                    },
                )
                .unwrap_err();
        }
    }
}