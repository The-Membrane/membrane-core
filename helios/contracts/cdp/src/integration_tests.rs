#[cfg(test)]
mod tests {
    
    use crate::helpers::{ LQContract, CDPContract };
        
    use cosmwasm_bignumber::Uint256;
    use membrane::positions::{ InstantiateMsg, QueryMsg, ExecuteMsg };
    use membrane::liq_queue::{ LiquidatibleResponse as LQ_LiquidatibleResponse};
    use membrane::stability_pool::{ LiquidatibleResponse as SP_LiquidatibleResponse, PoolResponse };
    use membrane::types::{AssetInfo, Asset, cAsset, LiqAsset};
    
    use osmo_bindings::{ SpotPriceResponse };
    use cosmwasm_std::{Addr, Coin, Empty, Uint128, Decimal, Response, StdResult, Binary, to_binary, coin, attr, StdError, CosmosMsg};
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor, BankKeeper};
    use schemars::JsonSchema;
    use serde::{ Deserialize, Serialize };

    //CDP Contract
    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        ).with_reply(crate::contract::reply);
        Box::new(contract)
    }

    //Mock LQ Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LQ_MockExecuteMsg {
        Liquidate {
            credit_price: Decimal, //Sent from Position's contract
            collateral_price: Decimal, //Sent from Position's contract
            collateral_amount: Uint256,
            bid_for: AssetInfo,
            bid_with: AssetInfo,   
            basket_id: Uint128,
            position_id: Uint128,
            position_owner: String, 
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct LQ_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LQ_MockQueryMsg {
        CheckLiquidatible {
            bid_for: AssetInfo,
            collateral_price: Decimal,
            collateral_amount: Uint256,
            credit_info: AssetInfo,
            credit_price: Decimal,
        }
    }


    pub fn liq_queue_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        match bid_for{
                            AssetInfo::Token { address: _ } => {
                                Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            },
                            AssetInfo::NativeToken { denom: _ } => {
                                assert_eq!(
                                vec![
                                    credit_price.to_string(),
                                    collateral_price.to_string(),
                                    collateral_amount.to_string(),
                                    bid_for.to_string(),
                                    bid_with.to_string(),
                                    basket_id.to_string(),
                                    position_id.to_string(),
                                    position_owner.to_string(),
                                ],vec![
                                    "1".to_string(),
                                    "1".to_string(),
                                    "2000".to_string(),
                                    "debit".to_string(),
                                    "credit".to_string(),
                                    "1".to_string(),
                                    "1".to_string(),
                                    "user".to_string(),
                                ]);


                                Ok(Response::new().add_attributes(vec![
                                    attr("action", "execute_bid"),
                                    attr("denom", bid_with.to_string()),
                                    attr("repay_amount", Uint128::new(2_000u128).to_string()),
                                    attr("collateral_token", bid_for.to_string()),
                                    attr("collateral_info", "native_token"),
                                    attr("collateral_amount", collateral_amount),
                                ]))
                            }
                        }
                    }
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "222".to_string(),
                                total_credit_repaid: "2000".to_string(),
                            })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn liq_queue_contract_errors()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    LQ_MockExecuteMsg::Liquidate {
                        credit_price,
                        collateral_price,
                        collateral_amount,
                        bid_for,
                        bid_with,
                        basket_id,
                        position_id,
                        position_owner,
                    } => {
                        Err( StdError::GenericErr { msg: "no siree".to_string() })
                    }
                }
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::CheckLiquidatible { 
                        bid_for, 
                        collateral_price, 
                        collateral_amount, 
                        credit_info, 
                        credit_price 
                    } => Ok(
                        to_binary(
                            &LQ_LiquidatibleResponse {
                                leftover_collateral: "222".to_string(),
                                total_credit_repaid: "2000".to_string(),
                            })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock SP Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockExecuteMsg {
        Liquidate {
            credit_asset: LiqAsset, 
        },
        Distribute {
            distribution_assets: Vec<cAsset>,
            credit_asset: AssetInfo,
            credit_price: Decimal,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct SP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SP_MockQueryMsg {
        CheckLiquidatible {
            asset: LiqAsset
        },
        AssetPool {
            asset_info: AssetInfo 
        },
    }

    pub fn stability_pool_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                        if credit_asset.to_string() != "222.222225 credit".to_string() && credit_asset.to_string() != "2000 credit".to_string() && credit_asset.to_string() != "22222.22225 credit".to_string(){
                            panic!("{}", credit_asset.to_string());
                        }
                        
                        Ok(Response::new()
                            .add_attribute("method", "liquidate")
                            .add_attribute("leftover_repayment", "0"))
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets, 
                        credit_asset, 
                        credit_price } => {
                        
                        if distribution_assets != vec![
                                cAsset { 
                                    asset: 
                                        Asset { 
                                            info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                            amount: Uint128::new(244) 
                                        }, 
                                    oracle: "funnybone".to_string(), 
                                    max_borrow_LTV: Decimal::percent(50), 
                                    max_LTV: Decimal::percent(70),}]
                            && distribution_assets != vec![
                                cAsset { 
                                    asset: 
                                        Asset { 
                                            info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                            amount: Uint128::new(2447) 
                                        }, 
                                    oracle: "funnybone".to_string(), 
                                    max_borrow_LTV: Decimal::percent(50), 
                                    max_LTV: Decimal::percent(70),}]
                            &&
                            distribution_assets != vec![
                                cAsset { 
                                    asset: 
                                        Asset { 
                                            info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                                            amount: Uint128::new(55000) 
                                        }, 
                                    oracle: "funnybone".to_string(), 
                                    max_borrow_LTV: Decimal::percent(50), 
                                    max_LTV: Decimal::percent(70),}]{
                                            assert_ne!(distribution_assets, distribution_assets);
                                        }

                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
                                amount: Uint128::zero(),
                            },
                            liq_premium: Decimal::percent(10),
                            deposits: vec![],
                        })?),
                }
            },
        );
        Box::new(contract)
    }

    pub fn stability_pool_contract_errors()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    SP_MockExecuteMsg::Liquidate {
                        credit_asset
                    } => {
                        
                        Err( StdError::GenericErr { msg: "no siree".to_string() })
                    }
                    SP_MockExecuteMsg::Distribute { 
                        distribution_assets, 
                        credit_asset, 
                        credit_price } => {

                                                
                        Ok(Response::new()
                            .add_attribute("method", "distribute")
                            .add_attribute("credit_asset", "cdl"))
                    },
                }
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::CheckLiquidatible { 
                        asset,
                    } => Ok(
                        to_binary(
                            &SP_LiquidatibleResponse {
                                leftover: Decimal::zero(),
                            })?),
                    SP_MockQueryMsg::AssetPool { asset_info 
                    } => Ok(
                        to_binary(&PoolResponse {
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken { denom: "cdl".to_string() },
                                amount: Uint128::zero(),
                            },
                            liq_premium: Decimal::percent(10),
                            deposits: vec![],
                        })?),
                }
            },
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
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {
        SpotPrice {
            asset: String,
        }
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
                        )
                    
                        }
                },
        );
        Box::new(contract)
    }

    //Mock Router Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockExecuteMsg {
        SwapFromNative {
            to: AssetInfo,
            max_spread: Option<Decimal>,
            recipient: Option<String>,
            hook_msg: Option<Binary>,
            split: Option<bool>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Router_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Router_MockQueryMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn router_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Router_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Router_MockExecuteMsg::SwapFromNative { 
                        to, 
                        max_spread, 
                        recipient, 
                        hook_msg, 
                        split } => {
                        
                        Ok(Response::default())
                    },
                }
            },
            |_, _, _, _: Router_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Router_MockQueryMsg| -> StdResult<Binary> { Ok( to_binary(&MockResponse {})? ) },
        );
        Box::new(contract)
    }

    


    const USER: &str = "user";
    const ADMIN: &str = "admin";
    // const NATIVE_DENOM: &str = "denom";

    // fn mock_app() -> App {
    //     AppBuilder::new().build(|router, _, storage| {
    //         router
    //             .bank
    //             .init_balance(
    //                 storage,
    //                 &Addr::unchecked(USER),
    //                 vec![Coin {
    //                     denom: NATIVE_DENOM.to_string(),
    //                     amount: Uint128::new(1),
    //                 }],
    //             )
    //             .unwrap();
    //     })
    // }

    fn mock_app() -> App {
            AppBuilder::new().build(|router, _, storage| {
                                    
                let bank = BankKeeper::new();

                bank.init_balance(storage, &Addr::unchecked(USER), vec![coin(100_000, "debit")])
                .unwrap();
                bank.init_balance(storage, &Addr::unchecked("contract0"), vec![coin(2225, "credit")])
                .unwrap(); //contract0 = Stability Pool contract
                bank.init_balance(storage, &Addr::unchecked("test"), vec![coin(50_000, "credit"), coin(100_000, "debit")])
                .unwrap(); 
                bank.init_balance(storage, &Addr::unchecked("sender"), vec![coin(50_000, "credit")])
                .unwrap(); 

                router
                    .bank = bank;
                    
            })
        }

    fn proper_instantiate( sp_error: bool, lq_error: bool ) -> (App, CDPContract, LQContract) {
        let mut app = mock_app();
        
        //Instanitate SP
        let mut sp_id: u64;
        if sp_error{
            sp_id = app.store_code(stability_pool_contract_errors());
        }else{
            sp_id = app.store_code(stability_pool_contract());
        }

        let sp_contract_addr = app
            .instantiate_contract(
                sp_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instanitate Router
        let router_id = app.store_code(router_contract());

        let router_contract_addr = app
            .instantiate_contract(
                router_id,
                Addr::unchecked(ADMIN),
                &Router_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LQ
        let mut lq_id: u64;
        if lq_error{
            lq_id = app.store_code(liq_queue_contract_errors());
        }else{
            lq_id = app.store_code(liq_queue_contract());
        }
        

        let lq_contract_addr = app
            .instantiate_contract(
                lq_id,
                Addr::unchecked(ADMIN),
                &LQ_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        let lq_contract = LQContract(lq_contract_addr);

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

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());

        let msg = 
            InstantiateMsg {
                owner: Some(ADMIN.to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::zero(),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                    cAsset {
                        asset:
                            Asset {
                                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                                amount: Uint128::zero(),
                            },
                    oracle: "funnybone".to_string(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    }]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some( sp_contract_addr.to_string() ),
                dex_router: Some( router_contract_addr.to_string() ),
                fee_collector:  Some( "fee_collector".to_string()),
                osmosis_proxy: Some( osmosis_proxy_contract_addr.to_string() ),
        };

        

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let cdp_contract = CDPContract(cdp_contract_addr);


        (app, cdp_contract, lq_contract)
    }

    



    mod cdp {
        use crate::ContractError;

        use super::*;
        use cosmwasm_std::{Event, from_binary};
        use membrane::positions::{ExecuteMsg, ConfigResponse, PropResponse, PositionResponse};

        #[test]
        fn withdrawal() {
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let fee_collector = res.fee_collector;
            

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            
            //Insolvent withdrawal error
            let msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::from(100_000u128)
                }],
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
                        
            //Successful attempt
            let withdrawal_msg = ExecuteMsg::Withdraw {
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                assets: vec![Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::from(90_000u128)
                }], 
            };

            let cosmos_msg = cdp_contract.call( withdrawal_msg, vec![] ).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(10000));

        }

        #[test]
        fn increase_debt__repay() {
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let fee_collector = res.fee_collector;
            

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
            
            //Insolvent position error 
            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_001u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();
        
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("50000"));

            //Successful repayment
            let repay_msg = ExecuteMsg::Repay { 
                basket_id: Uint128::from(1u128), 
                position_id: Uint128::from(1u128), 
                position_owner:  Some("test".to_string()), 
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![ coin(50_000, "credit") ]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("0"));

           
        }

        #[test]
        fn liq_repay() {

            let (mut app, cdp_contract, lq_contract) = proper_instantiate( false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let fee_collector = res.fee_collector;
            

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( "test".to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

             /////Liq Repay///
            /// 
            /// //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(50_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap();

            //Successful liquidation
            let msg = ExecuteMsg::Liquidate { 
                basket_id: Uint128::new(1u128), 
                position_id: Uint128::new(1u128), 
                position_owner: "test".to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Unauthorized
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::new(50_000),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("test"), cosmos_msg).unwrap_err();            


            //Send SP funds to liquidate
            app.send_tokens(Addr::unchecked("sender"), Addr::unchecked(sp_addr.clone()), &[ coin(50_000, "credit") ]).unwrap();

            //Successful LiqRepay
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::new(50_000),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![ coin(50_000, "credit") ]).unwrap();
            app.execute(Addr::unchecked(sp_addr), cosmos_msg).unwrap();  
            
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  "test".to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.credit_amount, String::from("0"));           

        }

        
        #[test]
        fn liquidate() {
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( false, false);

            let res: ConfigResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&QueryMsg::Config {} ).unwrap();
            let sp_addr = res.stability_pool;
            let router_addr = res.dex_router;
            let fee_collector = res.fee_collector;
            

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                credit_interest: None, 
                liq_queue: Some( lq_contract.addr().to_string() ) 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Call LiqRepay to mimic a successfull SP Liquidate call
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::new(222),
                    } };

            let cosmos_msg = cdp_contract.call(msg, vec![coin( 222, "credit")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97290));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(fee_collector.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin(2003, "credit"), coin( 244, "debit")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 2000, "debit")]);


            /////////SP Errors////
            /// 
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( true, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                credit_interest: None, 
                liq_queue: Some( lq_contract.addr().to_string() ) 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            
            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![coin( 222, "debit")]);

            //Assert fees were sent.
            assert_eq!(app.wrap().query_all_balances(fee_collector.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![coin( 2000, "debit")]);
            //Assert SP wasn't sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2225 , "credit")]);
            
            //////LQ Errors///
            /// 
            
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( false, true);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                credit_interest: None, 
                liq_queue: Some( lq_contract.addr().to_string() ) 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //app.wrap().query_wasm_smart(cdp_contract.addr(),QueryMsg:: )

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call LiqRepay to mimic a successfull SP Liquidate call by both the initial SP and then LQ reply
            let msg = ExecuteMsg::LiqRepay { 
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::new(2225),
                    } };

            let cosmos_msg = cdp_contract.call(msg, vec![coin( 2225, "credit")]).unwrap();
            app.execute( Addr::unchecked(sp_addr.clone()), cosmos_msg).unwrap();

            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97087));

            //Assert sell wall wasn't sent assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![]);

            //Assert fees were sent. 
            assert_eq!(app.wrap().query_all_balances(fee_collector.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 444, "debit")]);

            //Assert collateral to be liquidated was sent 
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2447 , "debit")]);
            //Assert LQ wasn't sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![]);
            

            //////All Errors/////
            /// 
                
            let (mut app, cdp_contract, lq_contract) = proper_instantiate( true, true);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket { 
                basket_id: Uint128::new(1u128), 
                added_cAsset: None, 
                owner: None, 
                credit_interest: None, 
                liq_queue: Some( lq_contract.addr().to_string() ) 
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            

            //Initial Deposit
            let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
            ];

            let msg = ExecuteMsg::Deposit { 
                assets,
                position_owner: Some( USER.to_string() ),
                basket_id: Uint128::from(1u128),
                position_id: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg, 
                    vec![
                        Coin { 
                            denom: "debit".to_string(),
                            amount: Uint128::from(100_000u128),
                            } 
                        ])
                    .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //app.wrap().query_wasm_smart(cdp_contract.addr(),QueryMsg:: )

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt{
                basket_id: Uint128::from(1u128),
                position_id: Uint128::from(1u128),
                amount: Uint128::from(5_000u128),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate { 
                    basket_id: Uint128::new(1u128), 
                    position_id: Uint128::new(1u128), 
                    position_owner: USER.to_string(), 
                };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetPosition { 
                position_id: Uint128::new(1u128), 
                basket_id: Uint128::new(1u128), 
                user:  USER.to_string(),  
            };
            let res: PositionResponse = app.wrap().query_wasm_smart(cdp_contract.addr(),&query_msg.clone() ).unwrap();
            assert_eq!(res.collateral_assets[0].asset.amount, Uint128::new(97312));

            //Assert sell wall was sent assets all Assets
            assert_eq!(app.wrap().query_all_balances(router_addr.clone()).unwrap(), vec![coin( 2222, "debit")]);

            //Assert fees were sent. In this case the Router is subbing in as the fee collector.
            assert_eq!(app.wrap().query_all_balances(fee_collector.clone()).unwrap(), vec![coin( 22, "debit")]);
            assert_eq!(app.wrap().query_all_balances(USER).unwrap(), vec![coin( 444, "debit")]);

            //Assert neither module was sent any due to the Error
            assert_eq!(app.wrap().query_all_balances(sp_addr.clone()).unwrap(), vec![coin( 2225 , "credit")]);
            assert_eq!(app.wrap().query_all_balances(lq_contract.addr()).unwrap(), vec![]);
        }

        
    }
}