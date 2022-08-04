
#[cfg(test)]
mod tests {
    use std::env;

    use crate::ContractError;
    use crate::contract::{execute, instantiate, query};
    use crate::state::CONFIG;

    use super::*;
    use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info, MockApi, MockQuerier};
    use cosmwasm_std::{coins, from_binary, attr, SubMsg, Uint128, Decimal, to_binary, CosmosMsg, WasmMsg, Coin, StdError, OwnedDeps, MemoryStorage, MessageInfo, Addr};
    use cw20::{ Cw20ExecuteMsg };

    use membrane::stability_pool::{ExecuteMsg, InstantiateMsg, ClaimsResponse, QueryMsg, DepositResponse, PoolResponse, Cw20HookMsg, LiquidatibleResponse};
    use membrane::positions::{ ExecuteMsg as CDP_ExecuteMsg, Cw20HookMsg as CDP_Cw20HookMsg };
    use membrane::apollo_router::{ ExecuteMsg as RouterExecuteMsg, Cw20HookMsg as RouterCw20HookMsg };
    use membrane::types::{AssetPool, Asset, AssetInfo, cAsset, LiqAsset, PositionUserInfo};

    #[test]
    fn deposit() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "instantiate"),
            attr("owner", "sender88"),
            ]
        );

        //Depositing 2 invalid asset
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "notcredit".to_string() }, AssetInfo::NativeToken { denom: "notnotnotcredit".to_string() } ],
            user: None,
        };

        let mut coinz = coins( 10, "notcredit" );
        coinz.extend( coins( 10, "notnotnotcredit" ) );

        let invalid_info = mock_info("sender88", &coinz);

        //Fail due to Invalid Asset
        let _err = execute(deps.as_mut(), mock_env(), invalid_info.clone(), deposit_msg).unwrap_err();

        //Query position data to make sure it was NOT saved to state 
        let res = query(deps.as_ref(),
        mock_env(),
        QueryMsg::AssetDeposits {
            user: "sender88".to_string(),
            asset_info: AssetInfo::NativeToken { denom: "notcredit".to_string() }
        });
        let error = "User has no open positions in this asset pool or the pool doesn't exist".to_string();
        
        match res {
            Err(StdError::GenericErr { msg: error }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Deposit should've failed due to an invalid asset"),
        } 

        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: None,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "deposit"),
            attr("position_owner","sender88"),
            attr("deposited_assets", "11 credit"),
            attr("deposited_assets", "11 2ndcredit"),
            ]
        );

        //Query position data to make sure it was saved to state correctly
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }
            })
            .unwrap();
        
        let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "2ndcredit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());

        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetPool {
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            })
            .unwrap();
        
        let resp: PoolResponse = from_binary(&res).unwrap();

        assert_eq!(resp.credit_asset.to_string(), "11 credit".to_string());
        assert_eq!(resp.liq_premium.to_string(), "0".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());


    }


    #[test]
    fn withdrawal() {
        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Depositing 2 asseta
        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();
        

        //Invalid Asset
        let assets: Vec<Asset> = vec![
           Asset { 
                info: AssetInfo::NativeToken { denom: "notcredit".to_string() }, 
                amount: Uint128::new(0u128) }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg);

         //Query position data to make sure nothing was withdrawn
         let res = query(deps.as_ref(),
         mock_env(),
         QueryMsg::AssetDeposits {
             user: "sender88".to_string(),
             asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
         }).unwrap();
     
         let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "credit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());
        /////////////////////

        //Invalid Withdrawal "Amount too high"
        let assets: Vec<Asset> = vec![
           Asset { 
                info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                amount: Uint128::new(12u128) }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let empty_info = mock_info("sender88", &[]);
        let res = execute(deps.as_mut(), mock_env(), empty_info, withdraw_msg);

        match res {
            Err(ContractError::InvalidWithdrawal {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Withdrawal amount too high, should've failed"),
        } 
        
        //Successful attempt
        let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(5u128),},
            Asset { 
                info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                amount: Uint128::new(5u128) }
            
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "withdraw"),
            attr("position_owner","sender88"),
            attr("withdrawn_assets", "5 credit"),
            attr("withdrawn_assets", "5 credit"),
            ]
        );

        //Query position data to make sure it was saved to state correctly
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            })
            .unwrap();
        
        let resp: DepositResponse = from_binary(&res).unwrap();

        assert_eq!(resp.asset.to_string(), "credit".to_string());
        assert_eq!(resp.deposits[0].to_string(), "sender88 1".to_string());

         //Successful attempt
         let assets: Vec<Asset> = vec![
            Asset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Uint128::from(1u128),
            }
        ];

        let withdraw_msg = ExecuteMsg::Withdraw { 
            assets,
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), withdraw_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "withdraw"),
            attr("position_owner","sender88"),
            attr("withdrawn_assets", "1 credit"),
            ]
        );

        //Query position data to make sure it was deleted from state 
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::AssetDeposits {
                user: "sender88".to_string(),
                asset_info: AssetInfo::NativeToken { denom: "credit".to_string() }
            });
    
        
        let error = "User has no open positions in this asset pool or the pool doesn't exist".to_string();
        
        match res {
            Err(StdError::GenericErr { msg: error }) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Deposit should've failed due to an invalid withdrawal amount"),
        }
        

    }

    #[test]
    fn liquidate(){

        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        let mut coin = coins(11, "credit");
        coin.append(&mut coins(11, "2ndcredit"));
        //Instantiating contract
        let info = mock_info("sender88", &coin);
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Depositing 2nd asset
        //Add Pool for a 2nd deposit asset
        let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: Asset { 
                    info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
                    amount: Uint128::zero() },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg).unwrap();

        //Successful attempt
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Unauthorized Sender
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::zero(),
            },  
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), liq_msg);

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Liquidation should have failed bc of an unauthorized sender"),
        } 


        //Invalid Credit Asset
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "notcredit".to_string() },
                amount: Decimal::zero(),
            }, 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg);

        match res {
            Err(ContractError::InvalidAsset {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Liquidation should have failed bc of an invalid credit asset"),
        } 

        //CheckLiquidatible
        let msg = QueryMsg::CheckLiquidatible { 
            asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(12u128, 1u128),
        }};
        let res = query( deps.as_ref(), mock_env(), msg ).unwrap();
        let resp: LiquidatibleResponse = from_binary(&res).unwrap();
        assert_eq!( resp.leftover.to_string(), String::from("1") );

        //Successful Attempt
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(1u128, 1u128),
            }, 
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "liquidate"),
            attr("leftover_repayment", "0 credit"),
        ]);

        let config = CONFIG.load(&deps.storage).unwrap();

        assert_eq!(
            res.messages,
            vec![SubMsg::new(
                CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.owner.to_string(),
                    funds: vec![Coin { 
                        denom: "credit".to_string(), 
                        amount: Uint128::new(1u128) 
                    }],
                    msg: to_binary(&CDP_ExecuteMsg::LiqRepay {
                        credit_asset:Asset{
                            info: AssetInfo::NativeToken{
                                denom:"credit".to_string()
                            },
                            amount:Uint128::from(1u128),
                        }, 
                    })
                    .unwrap(),
                })
            )]
        );

    }

    #[test]
    fn distribute(){

        let mut deps = mock_dependencies_with_balance(&coins(2, "credit"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        //Instantiating contract
        let info = mock_info("sender88", &coins(5, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        //Unauthorized Sender
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![],
            distribution_asset_ratios: vec![],
            credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
            distribute_for: Uint128::zero(),
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), distribute_msg);

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Distribution should have failed bc of an unauthorized sender"),
        } 

        //Invalid Credit Asset
        let distribute_msg = ExecuteMsg::Distribute { 
                distribution_assets: vec![
                    Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::new(100u128) }],
                distribution_asset_ratios: vec![ Decimal::percent(100) ],
                credit_asset: AssetInfo::NativeToken { denom: "notcredit".to_string() }, 
                distribute_for: Uint128::zero(),
        };

        let res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg);

        match res {
            Err(ContractError::InvalidAsset {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Distribution should've failed bc of an invalid credit asset"),
        } 

        //Deposit for first user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: None,
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Deposit for second user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: Some("2nduser".to_string()),
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Succesfful attempt
               
        //Liquidation
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(8u128, 1u128),
            }, 
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

        //Distribute
        let distribute_msg = ExecuteMsg::Distribute { 
                distribution_assets: vec![
                    Asset { 
                        info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                        amount: Uint128::new(100u128) },
                    Asset { 
                        info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                        amount: Uint128::new(100u128) }],
                distribution_asset_ratios: vec![ Decimal::percent(50), Decimal::percent(50) ],
                credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                distribute_for: Uint128::new(8),
        };
        
        let mut coin = coins(100, "debit");
        coin.append(&mut coins(100, "2nddebit"));

        let info = mock_info("sender88", &coin);

        let res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "distribute"),
            attr("credit_asset", "credit"),
            attr("distribution_assets", "100 debit"),
            attr("distribution_assets", "100 2nddebit"),
        ]);

        //Query and assert User claimables
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::UserClaims {
                user: "sender88".to_string(),
            }).unwrap();

         
        let resp: ClaimsResponse = from_binary(&res).unwrap();
        
        assert_eq!(resp.claims[0].to_string(), "100 debit".to_string());
        assert_eq!(resp.claims[1].to_string(), "25 2nddebit".to_string());

        //Query and assert User claimables
        let res = query(deps.as_ref(),
            mock_env(),
            QueryMsg::UserClaims {
                user: "2nduser".to_string(),
            }).unwrap();

         
        let resp: ClaimsResponse = from_binary(&res).unwrap();
        
        assert_eq!(resp.claims[0].to_string(), "75 2nddebit".to_string());
        
        
    }

    
    #[test]
    fn add_asset_pool(){

        let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

        let msg = InstantiateMsg {
                owner: Some("sender88".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        //Instantiating contract
        let info = mock_info("sender88", &coins(11, "credit"));
        let res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

         
        let credit_asset = Asset { 
            info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }, 
            amount: Uint128::zero() 
        };

         let add_msg = ExecuteMsg::AddPool { 
            asset_pool: AssetPool{
                credit_asset: credit_asset.clone(),
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }
        };

        let unauthorized_info = mock_info("notsender", &coins(0, "credit"));
        
        //Unauthorized Sender
        let res = execute(deps.as_mut(), mock_env(), unauthorized_info.clone(), add_msg.clone());

        match res {
            Err(ContractError::Unauthorized {}) => {},
            Err(_) => {panic!("{}", res.err().unwrap().to_string())},
            _ => panic!("Message should have failed bc of an unauthorized sender"),
        } 

         //Successful Attempt
        let res = execute(deps.as_mut(), mock_env(), info.clone(), add_msg.clone()).unwrap();

        assert_eq!(
            res.attributes,
            vec![
            attr("method", "add_asset_pool"),
            attr("asset","0 2ndcredit"),
            attr("premium", "0"),
        ]);


        //TODO: Add AssetPoolQuery
        let res = query(deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool {
            asset_info: AssetInfo::NativeToken { denom: "2ndcredit".to_string() }
        })
        .unwrap();
    
        let resp: PoolResponse = from_binary(&res).unwrap();

        assert_eq!( resp.credit_asset,  credit_asset);
        assert_eq!( resp.liq_premium,  Decimal::zero());

    }

    #[test]
    fn claims() {
         
        let mut deps = mock_dependencies_with_balance(&coins(2, "credit"));

        let msg = InstantiateMsg {
                owner: Some("owner00".to_string()),
                asset_pool: Some( AssetPool{
                    credit_asset: Asset { 
                        info: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                        amount: Uint128::zero() },
                    liq_premium: Decimal::zero(),
                    deposits: vec![],
                }),
                dex_router: Some( String::from("router_addr") ),
                max_spread: Some( Decimal::percent(10) ),
        };

        //Instantiating contract
        let info = mock_info("owner00", &coins(5, "credit"));
        let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

        
        //Deposit for first user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: Some("sender88".to_string()),
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Deposit for second user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: Some("2nduser".to_string()),
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();
        
        //Liquidation
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(8u128, 1u128),
            }, 
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

        //Distribute
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![
                Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::new(100u128) },
            Asset { 
                    info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                    amount: Uint128::new(100u128) }],
            distribution_asset_ratios: vec![ Decimal::percent( 50 ), Decimal::percent( 50 ) ],
            credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
            distribute_for: Uint128::new(8),
        };
        
        let mut coin = coins(100, "debit");
        coin.append(&mut coins(100, "2nddebit"));

        let info = mock_info("owner00", &coin);
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();


        //Error
        let claim_msg = ExecuteMsg::Claim { 
            claim_as_native: Some( String::from("credit") ), 
            claim_as_cw20: Some( String::from("protocol_token") ), 
            deposit_to: None, 
        }; //Can't claim as two different assets Error
        let err = execute(deps.as_mut(), mock_env(), info.clone(), claim_msg).unwrap_err();

        assert_eq!(err.to_string(), String::from("Custom Error val: \"Can't claim as multiple assets, if not all claimable assets\""));

        //Claim As Native
         let claim_msg = ExecuteMsg::Claim { 
            claim_as_native: Some( String::from("credit") ), 
            claim_as_cw20: None, 
            deposit_to: None, 
        };
        let info = mock_info("sender88", &[]);
        let res = execute(deps.as_mut(), mock_env(), info.clone(), claim_msg).unwrap();
        assert_eq!(res.messages, 
            vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(100, "debit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::NativeToken { denom: String::from("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("sender88") ), 
                    hook_msg: None, 
                    split: None, 
                }).unwrap(),
            })),
            
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(25, "2nddebit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::NativeToken { denom: String::from("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("sender88") ), 
                    hook_msg: None, 
                    split: None, 
                }).unwrap(),
            }))]
        );


        //Claim As Native + Deposit_to
        let claim_msg = ExecuteMsg::Claim { 
            claim_as_native: Some( String::from("credit") ), 
            claim_as_cw20: None, 
            deposit_to: Some( PositionUserInfo {
                basket_id: Uint128::new(1),
                position_id: Some( Uint128::new(1) ),
                position_owner: Some( String::from("sender88") ),
            } ), 
        };
        let info = mock_info( "2nduser", &[]);
        let res = execute(deps.as_mut(), mock_env(), info.clone(), claim_msg).unwrap();

        let deposit_msg = CDP_ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom:  String::from("credit") } ], 
            position_owner: Some( String::from("sender88") ), 
            basket_id:  Uint128::new(1), 
            position_id: Some( Uint128::new(1) ),
        };
        assert_eq!(res.messages, 
            vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(75, "2nddebit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::NativeToken { denom: String::from("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("owner00") ), 
                    hook_msg: Some( to_binary( &deposit_msg ).unwrap() ), 
                    split: None, 
                }).unwrap(),
            }))]
        );

        //////Reset State/////
        /// //Deposit for first user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: None,
        };
        let info = mock_info("sender88", &coins(5, "credit"));
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        //Deposit for second user
        let deposit_msg = ExecuteMsg::Deposit { 
            assets: vec![ AssetInfo::NativeToken { denom: "credit".to_string() }, AssetInfo::NativeToken { denom: "2ndcredit".to_string() } ],
            user: Some("2nduser".to_string()),
        };

        let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

        
        //Liquidation
        let liq_msg = ExecuteMsg::Liquidate { 
            credit_asset: LiqAsset {
                info: AssetInfo::NativeToken { denom: "credit".to_string() },
                amount: Decimal::from_ratio(8u128, 1u128),
            }, 
        };
        let info = mock_info("owner00", &[]);
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();
        
        
        //Distribute
        let distribute_msg = ExecuteMsg::Distribute { 
            distribution_assets: vec![
                Asset { 
                    info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
                    amount: Uint128::new(100u128) },
                Asset { 
                    info: AssetInfo::NativeToken { denom: "2nddebit".to_string() }, 
                    amount: Uint128::new(100u128) }],
                distribution_asset_ratios: vec![ Decimal::percent( 50 ), Decimal::percent( 50 ) ],
                credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
                distribute_for: Uint128::new(8),
        };
        
        let mut coin = coins(100, "debit");
        coin.append(&mut coins(100, "2nddebit"));

        let info = mock_info("owner00", &coin);
        let _res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();


        //Claim As Cw20
         let claim_msg = ExecuteMsg::Claim { 
            claim_as_native: None, 
            claim_as_cw20: Some( String::from("credit") ), 
            deposit_to: None, 
        };
        let info = mock_info("sender88", &[]);
        let res = execute(deps.as_mut(), mock_env(), info.clone(), claim_msg).unwrap();
        assert_eq!(res.messages, 
            vec![ SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(100, "debit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::Token { address: Addr::unchecked("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("sender88") ), 
                    hook_msg: None, 
                    split: None, 
                }).unwrap(),
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(25, "2nddebit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::Token { address: Addr::unchecked("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("sender88") ), 
                    hook_msg: None, 
                    split: None, 
                }).unwrap(),
            }))]
        );
        
        //Claim As Cw20 + Deposit_to
        let claim_msg = ExecuteMsg::Claim { 
            claim_as_native: None, 
            claim_as_cw20: Some( String::from("credit") ), 
            deposit_to: Some( PositionUserInfo {
                basket_id: Uint128::new(1),
                position_id: Some( Uint128::new(1) ),
                position_owner: Some( String::from("sender88") ),
            } ), 
        };
        let info = mock_info( "2nduser", &[]);
        let res = execute(deps.as_mut(), mock_env(), info.clone(), claim_msg).unwrap();

        let deposit_msg = CDP_Cw20HookMsg::Deposit { 
            position_owner: Some( String::from("sender88") ), 
            basket_id:  Uint128::new(1), 
            position_id: Some( Uint128::new(1) ),
        };
        assert_eq!(res.messages, 
            vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(75, "2nddebit"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative { 
                    to:  AssetInfo::Token { address: Addr::unchecked("credit") }, 
                    max_spread: Some( Decimal::percent(10) ), 
                    recipient: Some( String::from("owner00") ), 
                    hook_msg: Some( to_binary( &deposit_msg ).unwrap() ), 
                    split: None, 
                }).unwrap(),
            }))]
        );
    }

}

// //Distribute native token & Cw20, which needs to be sent as a hook
// let distribute_msg = Cw20HookMsg::Distribute { 
//     distribution_assets: vec![
//         Asset { 
//             info: AssetInfo::NativeToken { denom: "debit".to_string() }, 
//             amount: Uint128::new(100u128) }, 
//         Asset { 
//             info: AssetInfo::Token { address: Addr::unchecked("2nddebit") }, 
//             amount: Uint128::new(100u128) }],
//     distribution_asset_ratios: vec![ Decimal::percent( 50 ), Decimal::percent( 50 ) ],
//     credit_asset: AssetInfo::NativeToken { denom: "credit".to_string() }, 
//     credit_price: Decimal::from_ratio(Uint128::new(0u128), Uint128::new(1u128)), //This is 0 so we don't trigger Insufficient funds
// };

// let mut coin = coins(100, "debit");

// let cw20_send_msg = Cw20ExecuteMsg::Send {
//         contract: String::from("2nddebit"), 
//         amount: Uint128::new(100u128), 
//         msg: to_binary( &distribute_msg ).unwrap() 
//     };