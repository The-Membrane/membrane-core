

#[cfg(test)]
mod tests {
    use std::error::Error;

    use crate::ContractError;
    use crate::contract::{execute, instantiate, query};

    use super::*;
    use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary, attr, Uint128, Decimal, StdError, coin, to_binary, Addr};
    use cw20::Cw20ReceiveMsg;
    use membrane::positions::{ExecuteMsg, InstantiateMsg, PositionResponse, QueryMsg, PositionsResponse, BasketResponse, ConfigResponse, Cw20HookMsg};
    use membrane::types::{AssetInfo, Asset, cAsset, TWAPPoolInfo};
    use schemars::_serde_json::to_string;

    #[test]
    fn open_position_deposit(){
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("owner".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                },
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "2nddebit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                }  
            ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &vec![coin(11, "debit"), coin(11, "2nddebit") ]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();


        
        //No repayment price error {}
        let create_basket_msg = ExecuteMsg::CreateBasket {
            owner: Some("owner".to_string()),
            collateral_types: vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
            ],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(0u128),
            },
            credit_price: None,
            credit_interest: Some(Decimal::percent(1)),
            collateral_supply_caps: None,
            base_interest_rate: None,
            desired_debt_cap_util: None,
            credit_asset_twap_price_source:  TWAPPoolInfo { 
                pool_id: 0u64, 
                base_asset_denom: String::from("None"), 
                quote_asset_denom: String::from("None") 
            },
            credit_pool_ids: vec![],
            liquidity_multiplier_for_debt_caps: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), mock_info("owner", &[]), create_basket_msg).unwrap();

        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
        ];

        //Depositing into the basket that lacks a credit_price
        let deposit_msg = ExecuteMsg::Deposit {
            assets, 
            position_owner: Some(v_info.clone().sender.to_string()),
            basket_id: Uint128::from(2u128),
            position_id: None,
        };
        let info = mock_info("owner", &coins(11, "debit") );
        let res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg);
                
        match res{
            Err(ContractError::NoRepaymentPrice {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've error due to the basket not specifying a credit repayment price"),
        }

        //Testing Position creation

        //Invalid id test
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
                AssetInfo::NativeToken { denom: "2nddebit".to_string() },
        ];

        let error_exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: msg.clone().owner,
            basket_id: Uint128::from(1u128),
            position_id: Some(Uint128::from(3u128)),
        };

        //Fail due to a non-existent position
        //First msg deposits since no positions were initially found, meaning the _id never got tested
        let _res = execute(deps.as_mut(), mock_env(), v_info.clone(), error_exec_msg.clone());
        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), error_exec_msg);

        match res {
            Err(ContractError::NonExistentPosition {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Position deposit should've failed for passing in an invalid position ID"),
        } 


        //Fail for invalid collateral
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "fake_debit".to_string() },
        ];

        let info = mock_info("sender88", &coins(666, "fake_debit"));

        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: msg.clone().owner,
            basket_id: Uint128::from(1u128),
            position_id: None,
        };

        //fail due to invalid collateral
        let res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg);        

        match res {
            Err(ContractError::InvalidCollateral {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Position creation should've failed due to invalid cAsset type"),
        }

        //Successful attempt
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
                AssetInfo::NativeToken { denom: "2nddebit".to_string() },
        ];

        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: msg.clone().owner,
            basket_id: Uint128::from(1u128),
            position_id: None,
        };

        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), exec_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "deposit"),
            attr("basket_id", "1"),
            attr("position_owner","owner"),
            attr("position_id", "2"),
            attr("assets", "11 debit"),
            attr("assets", "11 2nddebit"),
            ]
        );

    }

    #[test]
    fn cw20_deposit(){
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
            owner: Some("owner".to_string()),
            credit_asset: Some(Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(0u128),
            }),
            credit_price: Some(Decimal::one()),
            collateral_types: Some(vec![
            cAsset {
                asset:
                    Asset {
                        info: AssetInfo::Token { address: Addr::unchecked("debit") },
                        amount: Uint128::from(0u128),
                    }, 
                debt_total: Uint128::zero(),
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(90),
                pool_info: None,
                pool_info_for_price:  TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                },
            },
            cAsset {
                asset:
                    Asset {
                        info: AssetInfo::Token { address: Addr::unchecked("2nddebit") },
                        amount: Uint128::from(0u128),
                    }, 
                debt_total: Uint128::zero(),
                max_borrow_LTV: Decimal::percent(50),
                max_LTV: Decimal::percent(70),
                pool_info: None,
                pool_info_for_price:  TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                },
            }  
        ]),
            credit_interest: Some(Decimal::percent(1)),
            liq_fee: Decimal::percent(1),
            stability_pool: Some("stability_pool".to_string()),
            dex_router: Some("router".to_string()),
            staking_contract: Some("staking_contract".to_string()),
            interest_revenue_collector: None,
            osmosis_proxy: Some("proxy".to_string()),
            debt_auction: Some( "debt_auction".to_string()),
            oracle_time_limit: 60u64,
            debt_minimum: Uint128::new(100u128),
            collateral_supply_caps: None,
            base_interest_rate: None,
            desired_debt_cap_util: None,
            twap_timeframe: 90u64,
            credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                pool_id: 0u64, 
                base_asset_denom: String::from("None"), 
                quote_asset_denom: String::from("None") 
            } ),
            credit_pool_ids: None,
            liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let v_info = mock_info("debit", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //Deposit
        let exec_msg = ExecuteMsg::Receive( Cw20ReceiveMsg {
            sender: String::from("sender88"),
            amount: Uint128::new(1),
            msg: to_binary(&Cw20HookMsg::Deposit{
                    position_owner: msg.clone().owner,
                    basket_id: Uint128::from(1u128),
                    position_id: None,       
            }).unwrap(),
        });
        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), exec_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "deposit"),
            attr("basket_id", "1"),
            attr("position_owner","owner"),
            attr("position_id", "1"),
            attr("assets", "1 debit"),
            ]
        );
        
    }

    #[test]
    fn withdrawal(){

        let mut deps     = mock_dependencies_with_balance(&coins(2, "token"));
        
        let msg = InstantiateMsg {
                owner: Some("owner".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                    } 
                ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();

        let valid_assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                amount: Uint128::from(5u128),
            }
        ];

        //User has no positions in the basket error
        let withdrawal_msg = ExecuteMsg::Withdraw {
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(1u128),
            assets: valid_assets.clone(), 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdrawal_msg);

        match res {
            Err(ContractError::NoUserPositions {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Position withdrawal should've failed due to having no positions in the passed basket"),
        }

         //Initial deposit
         let assets: Vec<AssetInfo> = vec![
            AssetInfo::NativeToken { denom: "debit".to_string() },
        ];

        let info = mock_info("sender88", &coins(11, "debit"));

        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: Some(info.clone().sender.to_string()),
            basket_id: Uint128::from(1u128),
            position_id: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg).unwrap();


        //Non-existent position error but user still has positions in the basket
        let withdrawal_msg = ExecuteMsg::Withdraw {
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(3u128),
            assets: vec![ Asset {
                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                amount: Uint128::zero(),
            }], 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdrawal_msg);

        match res {
            Err(ContractError::NonExistentPosition {}) => {},
            Err( err ) => {panic!("{}", err.to_string())},
            _ => panic!("Position withdrawal should've failed due to invalid position id"),
        }

        //Invalid collateral fail
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "notdebit".to_string() },
                amount: Uint128::from(10u128),
            }
        ];

        let withdrawal_msg = ExecuteMsg::Withdraw {
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(1u128),
            assets: assets.clone(), 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdrawal_msg);

        match res {
            Err(ContractError::InvalidCollateral {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Position withdrawal should've failed due to invalid cAsset type"),
        }
        
        // //Withdrawing too much error
        // let assets: Vec<Asset> = vec![
        //     Asset {
        //         info: AssetInfo::NativeToken { denom: "debit".to_string() },
        //         amount: Uint128::from(333333333u128),
        //     }
        // ];

        // let withdrawal_msg = ExecuteMsg::Withdraw {
        //     basket_id: Uint128::from(1u128),
        //     position_id: Uint128::from(1u128),
        //     assets: assets.clone(), 
        // };

        // let res = execute(deps.as_mut(), mock_env(), info.clone(), withdrawal_msg);

        // match res {
        //     Err(ContractError::InvalidWithdrawal {}) => {},
        //     Err(_) => {panic!("{}", res.err().unwrap().to_string())},
        //     _ => panic!("Position withdrawal should've failed due to invalid withdrawal amount"),
        // }
        
    }

    #[test]
    fn increase_debt() {
        
        let mut deps     = mock_dependencies_with_balance(&coins(2, "token"));
        
        let msg = InstantiateMsg {
                owner: Some("owner".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
                ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let info = mock_info("sender88", &coins(11, "debit"));
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();

        //NoUserPositions Error
        let increase_debt_msg = ExecuteMsg::IncreaseDebt{
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(1u128),
            amount: Uint128::from(1u128),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), increase_debt_msg);

        match res{
            Err(ContractError::NoUserPositions {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc no positions have been created yet"),
        }

        //No repayment price error {}
        let create_basket_msg = ExecuteMsg::CreateBasket {
            owner: Some("owner".to_string()),
            collateral_types: vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        },
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
            ],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(0u128),
            },
            credit_price: None,
            credit_interest: Some(Decimal::percent(1)),
            collateral_supply_caps: None,
            base_interest_rate: None,
            desired_debt_cap_util: None,
            credit_asset_twap_price_source:  TWAPPoolInfo { 
                pool_id: 0u64, 
                base_asset_denom: String::from("None"), 
                quote_asset_denom: String::from("None") 
            },
            credit_pool_ids: vec![],
            liquidity_multiplier_for_debt_caps: None,
        };
        let info = mock_info("owner", &coins(11, "debit"));
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), create_basket_msg).unwrap();

        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
        ];

        //Depositing into the basket that lacks a credit_price
        let deposit_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: Some(info.clone().sender.to_string()),
            basket_id: Uint128::from(2u128),
            position_id: None,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg);

        match res{
            Err(ContractError::NoRepaymentPrice {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc the basket has no repayment price"),
        } 
        
       
         //Initial deposit
        let assets: Vec<AssetInfo> = vec![
            AssetInfo::NativeToken { denom: "debit".to_string() },
        ];

        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: None,
            basket_id: Uint128::from(1u128),
            position_id: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg).unwrap();

        //NonExistentPosition Error
        let increase_debt_msg = ExecuteMsg::IncreaseDebt{
            basket_id: Uint128::from(1u128),
            position_id: Uint128::from(3u128),
            amount: Uint128::from(1u128),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), increase_debt_msg);

        match res{
            Err(ContractError::NonExistentPosition {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc no position under the _id has been created"),
        }

        //NonExistentBasket Error
        let increase_debt_msg = ExecuteMsg::IncreaseDebt{
            basket_id: Uint128::from(3u128),
            position_id: Uint128::from(1u128),
            amount: Uint128::from(1u128),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), increase_debt_msg);

        match res{
            Err(ContractError::NonExistentBasket {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc there is no basket under said _id"),
        }

    } 

    #[test]
    fn repay(){

        let mut deps     = mock_dependencies_with_balance(&coins(2, "token"));
        
        let msg = InstantiateMsg {
                owner: Some("owner".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
                ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("osmosis_proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &coins(1, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();


        //NoUserPositions Error
        let repay_msg = ExecuteMsg::Repay { 
            basket_id: Uint128::from(1u128), 
            position_id: Uint128::from(1u128), 
            position_owner:  Some(v_info.clone().sender.to_string()), 
        };

        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), repay_msg);

        match res{
            Err(ContractError::NoUserPositions {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc there are no open positions in this basket under the user's ownership"),
        }
        
        //Initial deposit
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
        ];

        let info = mock_info("sender88", &coins(11, "debit"));

        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: Some(info.clone().sender.to_string()),
            basket_id: Uint128::from(1u128),
            position_id: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg).unwrap();

        //Invalid Collateral Error
        let repay_msg = ExecuteMsg::Repay { 
            basket_id: Uint128::from(1u128), 
            position_id: Uint128::from(1u128), 
            position_owner:  Some(info.clone().sender.to_string()), 
        };

        let info = mock_info("sender88", &coins(1, "not_credit"));

        let res = execute(deps.as_mut(), mock_env(), info.clone(), repay_msg);

        match res{
            Err( err ) => { assert_eq!(err.to_string(), "Generic error: Incorrect denomination, sent asset denom and asset.info.denom differ".to_string())},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc the credit asset isn't correct for this basket"),
        }

        //NonExistent Basket Error
        let repay_msg = ExecuteMsg::Repay { 
            basket_id: Uint128::from(3u128), 
            position_id: Uint128::from(1u128), 
            position_owner:  Some(info.clone().sender.to_string()), 
        };

        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), repay_msg);

        match res{
            Err(ContractError::NonExistentBasket {  }) => {},
            Err( err ) => { panic!("{}", err.to_string()) },
            _ => panic!("This should've errored bc there is no basket under said _id"),
        }

        // //ExcessRepayment Error
        // let repay_msg = ExecuteMsg::Repay { 
        //     basket_id: Uint128::from(1u128), 
        //     position_id: Uint128::from(1u128), 
        //     position_owner:  Some(info.clone().sender.to_string()), 
        // };

        // let info = mock_info("sender88", &coins(333333, "credit"));

        // let res = execute(deps.as_mut(), mock_env(), info.clone(), repay_msg);

        // match res{
        //     Err(ContractError::ExcessRepayment {  }) => {},
        //     Err(_) => {panic!("{}", res.err().unwrap().to_string())},
        //     _ => panic!("This should've errored bc the credit amount is more than the open loan amount"),
        // }

        //NonExistent Position Error
        let repay_msg = ExecuteMsg::Repay { 
            basket_id: Uint128::from(1u128), 
            position_id: Uint128::from(3u128), 
            position_owner:  Some(info.clone().sender.to_string()), 
        };

        let res = execute(deps.as_mut(), mock_env(), v_info.clone(), repay_msg);

        match res{
            Err(ContractError::NonExistentPosition {  }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("This should've errored bc the position_id passed is non existent under this basket"),
        }
        
    }

    #[test]
    fn misc_query() {

        let mut deps     = mock_dependencies_with_balance(&coins(2, "token"));
        
        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
                ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("osmosis_proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &coins(1, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //Edit Admin
        let msg = ExecuteMsg::EditAdmin { owner: String::from("owner") };
        let _res = execute(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //Add 2ndary basket
        let create_basket_msg = ExecuteMsg::CreateBasket {
            owner: Some("owner".to_string()),
            collateral_types: vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        },
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
            ],
            credit_asset: Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(0u128),
            },
            credit_price: Some( Decimal::percent(100) ),
            credit_interest: Some(Decimal::percent(1)),
            collateral_supply_caps: None,
            base_interest_rate: None,
            desired_debt_cap_util: None,
            credit_asset_twap_price_source:  TWAPPoolInfo { 
                pool_id: 0u64, 
                base_asset_denom: String::from("None"), 
                quote_asset_denom: String::from("None") 
            },
            credit_pool_ids: vec![],
            liquidity_multiplier_for_debt_caps: None,
        };
        let v_info = mock_info("owner", &[] );
        let _res = execute(deps.as_mut(), mock_env(), v_info.clone(), create_basket_msg).unwrap();

        //Initial deposit to Basket 1
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
        ];
        
        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: Some( String::from("sender88") ),
            basket_id: Uint128::from(1u128),
            position_id: None,
        };
        let info = mock_info("sender88", &coins(11, "debit"));
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg).unwrap();

        //Initial deposit to Basket 2
        let assets: Vec<AssetInfo> = vec![
                AssetInfo::NativeToken { denom: "debit".to_string() },
        ];
        
        let exec_msg = ExecuteMsg::Deposit { 
            assets,
            position_owner: Some( String::from("sender88") ),
            basket_id: Uint128::from(2u128),
            position_id: None,
        };
        let info = mock_info("sender88", &coins(11, "debit"));
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), exec_msg).unwrap();

                
        //Query AllBaskets
        let msg = QueryMsg::GetAllBaskets { 
            start_after: None,
            limit: None,
        };
        let res = query( deps.as_ref(), mock_env(), msg)
        .unwrap();

        let resp: Vec<BasketResponse> = from_binary(&res).unwrap();
        assert_eq!( resp[0].basket_id, String::from(Uint128::from(1u128)) );
        assert_eq!( resp[1].basket_id, String::from(Uint128::from(2u128)) );
        assert_eq!(resp.len().to_string(), String::from("2"));   

    }

    #[test]
    fn edit_cAsset() {

        let mut deps     = mock_dependencies_with_balance(&coins(2, "token"));
        
        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                credit_asset: Some(Asset {
                    info: AssetInfo::NativeToken { denom: "credit".to_string() },
                    amount: Uint128::from(0u128),
                }),
                credit_price: Some(Decimal::one()),
                collateral_types: Some(vec![
                cAsset {
                    asset:
                        Asset {
                            info: AssetInfo::NativeToken { denom: "debit".to_string() },
                            amount: Uint128::from(0u128),
                        }, 
                    debt_total: Uint128::zero(),
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(90),
                    pool_info: None,
                    pool_info_for_price:  TWAPPoolInfo { 
                        pool_id: 0u64, 
                        base_asset_denom: String::from("None"), 
                        quote_asset_denom: String::from("None") 
                    },
                       } 
                ]),
                credit_interest: Some(Decimal::percent(1)),
                liq_fee: Decimal::percent(1),
                stability_pool: Some("stability_pool".to_string()),
                dex_router: Some("router".to_string()),
                staking_contract: Some("staking_contract".to_string()),
                interest_revenue_collector: None,
                osmosis_proxy: Some("osmosis_proxy".to_string()),
                debt_auction: Some( "debt_auction".to_string()),
                oracle_time_limit: 60u64,
                debt_minimum: Uint128::new(100u128),
                collateral_supply_caps: None,
                base_interest_rate: None,
                desired_debt_cap_util: None,
                twap_timeframe: 90u64,
                credit_asset_twap_price_source: Some( TWAPPoolInfo { 
                    pool_id: 0u64, 
                    base_asset_denom: String::from("None"), 
                    quote_asset_denom: String::from("None") 
                } ),
                credit_pool_ids: None,
                liquidity_multiplier_for_debt_caps: None,
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &coins(1, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();
        
        //Invalid Basket
        let edit_msg = ExecuteMsg::EditcAsset { 
            basket_id: Uint128::new(0u128), 
            asset: AssetInfo::NativeToken { denom: "debit".to_string() }, 
            max_borrow_LTV: None, 
            max_LTV: None, 
        };

        let info = mock_info("sender88", &[]);
        let _err = execute(deps.as_mut(), mock_env(), info.clone(), edit_msg).unwrap_err();

        //Invalid Asset
        let edit_msg = ExecuteMsg::EditcAsset { 
            basket_id: Uint128::new(1u128), 
            asset: AssetInfo::NativeToken { denom: "not_debit".to_string() }, 
            max_borrow_LTV: None, 
            max_LTV: None,  
        };

        let info = mock_info("sender88", &[]);
        let _err = execute(deps.as_mut(), mock_env(), info.clone(), edit_msg).unwrap_err();

        //Successfull edit
        let edit_msg = ExecuteMsg::EditcAsset { 
            basket_id: Uint128::new(1u128), 
            asset: AssetInfo::NativeToken { denom: "debit".to_string() }, 
            max_borrow_LTV: Some( Decimal::percent(99) ), 
            max_LTV: Some( Decimal::percent(100) ), 
        };

        let info = mock_info("sender88", &[]);
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), edit_msg).unwrap();
       
        //Query Basket
        let res = query( deps.as_ref(), mock_env(), QueryMsg::GetBasket { basket_id: Uint128::new(1u128) })
        .unwrap();

        let resp: BasketResponse = from_binary(&res).unwrap();
        assert_eq!( resp.collateral_types[0].max_borrow_LTV,  Decimal::percent(99) );
        assert_eq!( resp.collateral_types[0].max_LTV,  Decimal::percent(100) );
    }
}
