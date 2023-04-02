use crate::contract::{execute, instantiate, query};
use crate::state::CONFIG;
use crate::ContractError;

use cosmwasm_std::testing::{
    mock_dependencies, mock_dependencies_with_balance, mock_env, mock_info,
};
use cosmwasm_std::{
    attr, coins, from_binary, to_binary, Addr, Coin, CosmosMsg, BankMsg, Decimal, SubMsg, Uint128,
    WasmMsg,
};

use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;
use membrane::stability_pool::{
    Config, ClaimsResponse, ExecuteMsg, InstantiateMsg, LiquidatibleResponse,
    QueryMsg, DepositPositionResponse, UpdateConfig
};
use membrane::types::{Asset, AssetInfo, AssetPool, Deposit, UserInfo};

#[test]
fn deposit() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        positions_contract: String::from("positions_contract"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    let mut coin = coins(11, "credit");
    coin.append(&mut coins(11, "2ndcredit"));

    //Instantiating contract
    let info = mock_info("sender88", &coin);
    let res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    let resp = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Config {},
    )
    .unwrap();
    let resp: Config = from_binary(&resp).unwrap();
    assert_eq!(
        res.attributes,
        vec![attr("method", "instantiate"), attr("config", format!("{:?}", resp)), attr("contract_address", "cosmos2contract")]
    );

    //Depositing an invalid asset: Error
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let mut coinz = coins(10, "notcredit");
    coinz.extend(coins(10, "notnotnotcredit"));

    let invalid_info = mock_info("sender88", &coinz);
    let _err = execute(deps.as_mut(), mock_env(), invalid_info, deposit_msg).unwrap_err();

    //Query position data to make sure it was NOT saved to state
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: Some(String::from("sender88")),
            deposit_limit: None,
            start_after: None,
        }
    ).unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();
    if resp.deposits != vec![]{
        panic!("State wasn't saved correctly");
    }

    //Depositing below minimum: Error
    let mut minimum_coin = coins(4, "credit");
    let minimum_info = mock_info("sender88", &minimum_coin);
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let err = execute(deps.as_mut(), mock_env(), minimum_info, deposit_msg).unwrap_err();
    match err {
        ContractError::MinimumDeposit { min } => assert_eq!(min, Uint128::new(5)),
        _ => panic!("Unexpected error: {:?}", err),
    }

    // Deposit too many assets: Error
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let err = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap_err();
    
    //Successful attempt
    let mut coin = coins(11, "credit");
    let info = mock_info("sender88", &coin);
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "deposit"),
            attr("position_owner", "sender88"),
            attr("deposited_asset", "Asset { info: NativeToken { denom: \"credit\" }, amount: Uint128(11) }"),
        ]
    );

    //Query position data to make sure it was saved to state correctly
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: Some(String::from("sender88")),
            deposit_limit: None,
            start_after: None, },
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();

    assert_eq!(resp.credit_asset.to_string(), "11 credit".to_string());
    assert_eq!(resp.liq_premium.to_string(), "0".to_string());
    assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());
}

//#[test]
#[allow(dead_code)]
fn withdrawal() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5),
    };

    //Instantiating contract
    let info = mock_info("sender88", &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Successful "credit" deposit
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("sender88", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Successful "credit" deposit
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("sender88", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    //Query position data to make sure nothing was withdrawn
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: Some(String::from("sender88")),
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();
    assert_eq!(resp.deposits[0].to_string(), "sender88 11".to_string());
    /////////////////////

    //Invalid Withdrawal "Amount too high"
    let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(24u128) };
    let empty_info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), empty_info, withdraw_msg);

    match res {
        Err(ContractError::InvalidWithdrawal {}) => {}
        Err(_) => {
            panic!("{}", res.err().unwrap().to_string())
        }
        _ => panic!("Withdrawal amount too high, should've failed"),
    }
   
    //Successful Withdraw
    let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::from(12u128) };

    //First msg is to begin unstaking
    execute(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        withdraw_msg,
    )
    .unwrap();

    //Query to make sure the remaining amount of the 2nd "credit" deposit is still staked
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();
    assert_eq!(
        resp.deposits[1..=2],
        vec![
            Deposit {
                user: Addr::unchecked("sender88"),
                amount: Decimal::percent(1_00),
                deposit_time: mock_env().block.time.seconds(),
                last_accrued: mock_env().block.time.seconds(),
                unstake_time: Some(mock_env().block.time.seconds()),
            },
            Deposit {
                user: Addr::unchecked("sender88"),
                amount: Decimal::percent(10_00),
                deposit_time: mock_env().block.time.seconds(),
                last_accrued: mock_env().block.time.seconds(),
                unstake_time: None,
            }
        ]
    );

    //Restake
    let restake_msg = ExecuteMsg::Restake { restake_amount: Decimal::percent(12_00) };
    execute(deps.as_mut(), mock_env(), info.clone(), restake_msg).unwrap();

    //Successful ReWithdraw
    let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::from(12u128) };

    //First msg is to begin unstaking
    let res = execute(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        withdraw_msg.clone(),
    )
    .unwrap();
    //Assert none withdrawn which means Successful restake
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "withdraw"),
            attr("position_owner", "sender88"),
        ]
    );

    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(86400u64); //Added a day
    //Second msg to withdraw
    let res = execute(deps.as_mut(), env.clone(), info.clone(), withdraw_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "withdraw"),
            attr("position_owner", "sender88"),
            attr("withdrawn_asset", "12 credit"),
        ]
    );

    //Query position data to make sure it was saved to state correctly
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();
    let resp: AssetPool = from_binary(&res).unwrap();
    assert_eq!(resp.deposits[0].to_string(), "sender88 10".to_string());

    //Successful attempt
    let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::from(10u128) };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), withdraw_msg.clone()).unwrap();

    //Query position data to make sure it was deleted from state
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    ).unwrap();
    let resp: AssetPool = from_binary(&res).unwrap();
    
    if resp.deposits.into_iter().any(|deposit| deposit.user.to_string() == String::from("sender88")){
        panic!("State wasn't deleted correctly");
    }
}

#[test]
fn liquidate() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "token"));

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    let mut coin = coins(11, "credit");
    coin.append(&mut coins(11, "2ndcredit"));
    //Instantiating contract
    let info = mock_info("sender88", &coin);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Successful attempt
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Unauthorized Sender
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::zero() };

    let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

    let res = execute(
        deps.as_mut(),
        mock_env(),
        unauthorized_info,
        liq_msg,
    );

    match res {
        Err(ContractError::Unauthorized {}) => {}
        Err(_) => {
            panic!("{}", res.err().unwrap().to_string())
        }
        _ => panic!("Liquidation should have failed bc of an unauthorized sender"),
    }
    
    //CheckLiquidatible
    let msg = QueryMsg::CheckLiquidatible { amount: Decimal::from_ratio(12u128, 1u128) };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!(resp.leftover.to_string(), String::from("1"));

    //Successful Attempt
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(12u128, 1u128) };
    let cdp_info = mock_info("positions_contract", &vec![]);
    let res = execute(deps.as_mut(), mock_env(), cdp_info, liq_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "liquidate"),
            attr("leftover_repayment", "1 credit"),
        ]
    );

    let config = CONFIG.load(&deps.storage).unwrap();

    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.positions_contract.to_string(),
            funds: vec![Coin {
                denom: "credit".to_string(),
                amount: Uint128::new(11u128)
            }],
            msg: to_binary(&CDP_ExecuteMsg::LiqRepay {}).unwrap(),
        }))]
    );
}

#[test]
fn liquidate_bignums() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    let mut coin = coins(11_000_000_000_000, "credit");
    coin.append(&mut coins(11_000_000_000_000, "2ndcredit"));
    //Instantiating contract
    let info = mock_info("sender88", &coin);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Successful attempt
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();
    
    //CheckLiquidatible
    let msg = QueryMsg::CheckLiquidatible { amount: Decimal::from_ratio(12_000_000_000_000u128, 1u128) };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!(resp.leftover.to_string(), String::from("1000000000000"));

    //Successful Attempt
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(11_000_000_000_000u128, 1u128) };
    let cdp_info = mock_info("positions_contract", &coin);
    let res = execute(deps.as_mut(), mock_env(), cdp_info, liq_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "liquidate"),
            attr("leftover_repayment", "0 credit"),
        ]
    );

    let config = CONFIG.load(&deps.storage).unwrap();

    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.positions_contract.to_string(),
            funds: vec![Coin {
                denom: "credit".to_string(),
                amount: Uint128::new(11_000_000_000_000u128)
            }],
            msg: to_binary(&CDP_ExecuteMsg::LiqRepay {}).unwrap(),
        }))]
    );
}

#[test]
fn distribute() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "credit"));

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("user", &coins(5, "credit"));
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Unauthorized Sender
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![],
        distribution_asset_ratios: vec![],
        distribute_for: Uint128::zero(),
    };

    let unauthorized_info = mock_info("not_positions_contract", &coins(0, "credit"));

    let res = execute(
        deps.as_mut(),
        mock_env(),
        unauthorized_info,
        distribute_msg,
    );

    match res {
        Err(ContractError::Unauthorized {}) => {}
        Err(_) => {
            panic!("{}", res.err().unwrap().to_string())
        }
        _ => panic!("Distribution should have failed bc of an unauthorized sender"),
    }

    

    //Deposit for first user
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    //Deposit for second user
    let deposit_msg = ExecuteMsg::Deposit { user: Some(String::from("2nduser")) };
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Successful attempt
    //Liquidation
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(8u128, 1u128) };
    let cdp_info = mock_info("positions_contract", &vec![]);
    let _res = execute(deps.as_mut(), mock_env(), cdp_info, liq_msg).unwrap();

    //Distribute
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "2nddebit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
        ],
        distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
        distribute_for: Uint128::new(8),
    };

    let mut coin = coins(100, "debit");
    coin.append(&mut coins(100, "2nddebit"));

    let cdp_info = mock_info("positions_contract", &coin);

    let res = execute(deps.as_mut(), mock_env(), cdp_info, distribute_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "distribute"),
            attr("credit_asset", "credit"),
            attr("distribution_assets", "[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(100) }, Asset { info: NativeToken { denom: \"2nddebit\" }, amount: Uint128(100) }]"),
        ]
    );

    //Query and assert User claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "user".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(resp.claims[0].to_string(), "100 debit".to_string());
    assert_eq!(resp.claims[1].to_string(), "25 2nddebit".to_string());

    //Query and assert 2ndUser claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "2nduser".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(resp.claims[0].to_string(), "75 2nddebit".to_string());

    //Query position data to make sure 0 is leftover for "user"
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { 
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();
    let resp: AssetPool = from_binary(&res).unwrap();
    if resp.deposits.into_iter().any(|deposit| deposit.user.to_string() == String::from("user")){
        panic!("State wasn't deleted correctly");
    }

    //Query position data to make sure 2 is leftover for "2nduser"
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool {
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();
    assert_eq!(resp.deposits[0].to_string(), String::from("2nduser 2"));

    //2nd Liquidation
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(2u128, 1u128) };
    let cdp_info = mock_info("positions_contract", &vec![]);
    let _res = execute(deps.as_mut(), mock_env(), cdp_info, liq_msg).unwrap();

    //2nd Distribute to only th 2nduser
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "2nddebit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
        ],
        distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
        distribute_for: Uint128::new(2),
    };
    
    let mut coin = coins(100, "debit");
    coin.append(&mut coins(100, "2nddebit"));

    let cdp_info = mock_info("positions_contract", &coin);

    let res = execute(deps.as_mut(), mock_env(), cdp_info, distribute_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "distribute"),
            attr("credit_asset", "credit"),
            attr("distribution_assets", "[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(100) }, Asset { info: NativeToken { denom: \"2nddebit\" }, amount: Uint128(100) }]"),
        ]
    );

    //Query and assert 2ndUser claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "2nduser".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(
        resp.claims,
        vec![            
            Asset {
                info: AssetInfo::NativeToken { denom: "2nddebit".to_string() },
                amount: Uint128::new(175)
            },
            Asset {
                info: AssetInfo::NativeToken { denom: "debit".to_string() },
                amount: Uint128::new(100)
            }
        ],
    )

}

#[test]
fn distribute_bignums() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("sender88", &coins(5_000_000_000_000, "credit"));
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Unauthorized Sender
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![],
        distribution_asset_ratios: vec![],
        distribute_for: Uint128::zero(),
    };

    let unauthorized_info = mock_info("notsender", &coins(0, "credit"));

    let res = execute(
        deps.as_mut(),
        mock_env(),
        unauthorized_info,
        distribute_msg,
    );

    match res {
        Err(ContractError::Unauthorized {}) => {}
        Err(_) => {
            panic!("{}", res.err().unwrap().to_string())
        }
        _ => panic!("Distribution should have failed bc of an unauthorized sender"),
    }

    //Deposit for first user
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    //Deposit for second user
    let deposit_msg = ExecuteMsg::Deposit { user: Some("2nduser".to_string()) };
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Succesfful attempt
    let cdp_info = mock_info("positions_contract", &vec![]);
    //Liquidation
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(8_000_000_000_000u128, 1u128) };
    let _res = execute(deps.as_mut(), mock_env(), cdp_info, liq_msg).unwrap();

    //Distribute
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::new(100_000_000_000_000u128),
            },
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "2nddebit".to_string(),
                },
                amount: Uint128::new(100_000_000_000_000u128),
            },
        ],
        distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
        distribute_for: Uint128::new(8_000_000_000_000),
    };

    let mut coin = coins(100_000_000_000_000, "debit");
    coin.append(&mut coins(100_000_000_000_000, "2nddebit"));

    let info = mock_info("positions_contract", &coin);

    let res = execute(deps.as_mut(), mock_env(), info, distribute_msg).unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("method", "distribute"),
            attr("credit_asset", "credit"),
            attr("distribution_assets", "[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(100000000000000) }, Asset { info: NativeToken { denom: \"2nddebit\" }, amount: Uint128(100000000000000) }]"),
        ]
    );

    //Query and assert User claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "sender88".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(
        resp.claims[0].to_string(),
        "100000000000000 debit".to_string()
    );
    assert_eq!(
        resp.claims[1].to_string(),
        "25000000000000 2nddebit".to_string()
    );

    //Query and assert User claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "2nduser".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(
        resp.claims[0].to_string(),
        "75000000000000 2nddebit".to_string()
    );

    //Query position data to assert leftovers
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool {
            user: None,
            deposit_limit: None,
            start_after: None,
        }
    )
    .unwrap();
    let resp: AssetPool = from_binary(&res).unwrap();
    
    //Assert 2nduser leftover is leftover
    assert_eq!(
        resp.deposits[0].to_string(),
        String::from("2nduser 2000000000000")
    );
    //Assert sender88 has nothing
    if resp.deposits.into_iter().any(|deposit| deposit.user.to_string() == String::from("sender88")){
        panic!("State wasn't deleted correctly");
    }
}

#[test]
fn claims() {
    let mut deps = mock_dependencies_with_balance(&coins(2, "credit"));

    let msg = InstantiateMsg {
        owner: Some("owner00".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("owner00", &coins(5, "credit"));
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //Deposit for first user
    let deposit_msg = ExecuteMsg::Deposit { user: Some("sender88".to_string()) };
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    //Deposit for second user
    let deposit_msg = ExecuteMsg::Deposit { user: Some("2nduser".to_string()) };
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Liquidation
    let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(8u128, 1u128) };
    let cdp_info = mock_info("positions_contract", &coins(5, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), cdp_info.clone(), liq_msg).unwrap();

    //Distribute
    let distribute_msg = ExecuteMsg::Distribute {
        distribution_assets: vec![
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
            Asset {
                info: AssetInfo::NativeToken {
                    denom: "2nddebit".to_string(),
                },
                amount: Uint128::new(100u128),
            },
        ],
        distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
        distribute_for: Uint128::new(8),
    };

    let mut coin = coins(100, "debit");
    coin.append(&mut coins(100, "2nddebit"));

    let info = mock_info("positions_contract", &coin);
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();
   
    //Claim
    let claim_msg = ExecuteMsg::ClaimRewards {};
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, claim_msg).unwrap();

    assert_eq!(res.messages, vec![
        SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: "sender88".to_string(),
            amount: vec![Coin {
                denom: "debit".to_string(),
                amount: Uint128::new(100u128)
            },
            Coin {
                denom: "2nddebit".to_string(),
                amount: Uint128::new(25u128)
            }],
        }))
    ]);
    
    //Claim: Error, nothing to claim
    let claim_msg = ExecuteMsg::ClaimRewards {};
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, claim_msg).unwrap();
    assert_eq!(res.attributes[2].value, "[]".to_string());

    //Claim
    let claim_msg = ExecuteMsg::ClaimRewards {};
    let info = mock_info("2nduser", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, claim_msg).unwrap();

    assert_eq!(res.messages, vec![
        SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: "2nduser".to_string(),
            amount: vec![Coin {
                denom: "2nddebit".to_string(),
                amount: Uint128::new(75u128)
            }],
        }))
    ]);
}


fn cdp_repay() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Deposit
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("sender88", &coins(5_000_000_000_000, "credit"));
    let res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Repay: Error( Unauthorized )
    let repay_msg = ExecuteMsg::Repay {
        user_info: UserInfo {
            position_id: Uint128::new(1u128),
            position_owner: String::from("sender88"),
        },
        repayment: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("credit"),
            },
            amount: Uint128::new(1u128),
        },
    };
    let info = mock_info("sender88", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, repay_msg).unwrap_err();
    if let ContractError::Unauthorized {} = err {
    } else {
        panic!("{}", err.to_string());
    };

    //Repay: Error( InvalidAsset )
    let repay_msg = ExecuteMsg::Repay {
        user_info: UserInfo {
            position_id: Uint128::new(1u128),
            position_owner: String::from("sender88"),
        },
        repayment: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("invalid"),
            },
            amount: Uint128::new(1u128),
        },
    };
    let info = mock_info("positions_contract", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, repay_msg).unwrap_err();
    if let ContractError::InvalidAsset {} = err {
        /////
    } else {
        panic!("{}", err.to_string());
    };

    //Repay: Error( InvalidWithdrawal )
    //No funds
    let repay_msg = ExecuteMsg::Repay {
        user_info: UserInfo {
            position_id: Uint128::new(1u128),
            position_owner: String::from("no_funds"),
        },
        repayment: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("credit"),
            },
            amount: Uint128::new(0u128),
        },
    };
    let info = mock_info("positions_contract", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, repay_msg).unwrap_err();
    if let ContractError::InvalidWithdrawal {} = err {
        /////
    } else {
        panic!("{}", err.to_string());
    };

    //Repay: Error( InvalidWithdrawal )
    //Repayment above total user deposits
    let repay_msg = ExecuteMsg::Repay {
        user_info: UserInfo {
            position_id: Uint128::new(1u128),
            position_owner: String::from("no_funds"),
        },
        repayment: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("credit"),
            },
            amount: Uint128::new(1u128),
        },
    };
    let info = mock_info("positions_contract", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, repay_msg).unwrap_err();
    if let ContractError::InvalidWithdrawal {} = err {
        /////
    } else {
        panic!("{}", err.to_string());
    };

    //Repay: Success
    let repay_msg = ExecuteMsg::Repay {
        user_info: UserInfo {
            position_id: Uint128::new(1u128),
            position_owner: String::from("sender88"),
        },
        repayment: Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("credit"),
            },
            amount: Uint128::new(5_000_000_000_000u128),
        },
    };
    let info = mock_info("positions_contract", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, repay_msg).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: String::from("positions_contract"),
            funds: coins(5_000_000_000_000, "credit"),
            msg: to_binary(&CDP_ExecuteMsg::Repay {
                position_id: Uint128::new(1u128),
                position_owner: Some(String::from("sender88")),
                send_excess_to: Some(String::from("sender88")),
            })
            .unwrap(),
        }))]
    );

    //Assert State saved correctly
    //Query AssetPool
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool {
            user: None,
            deposit_limit: None,
            start_after: None,
        },
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();

    assert_eq!(resp.credit_asset.to_string(), "0 credit".to_string());
    assert_eq!(resp.liq_premium.to_string(), "0".to_string());
    assert_eq!(resp.deposits.len().to_string(), "0".to_string());
}

#[test]
fn update_config(){

    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    
    let msg = ExecuteMsg::UpdateConfig(UpdateConfig { 
        owner: Some(String::from("new_owner")),
        incentive_rate: Some(Decimal::one()), 
        max_incentives: Some(Uint128::new(100)), 
        unstaking_period: Some(1),  
        minimum_deposit_amount: Some(Uint128::new(10)),
        osmosis_proxy: Some(String::from("new_op")), 
        positions_contract: Some(String::from("new_cdp")), 
        mbrn_denom: Some(String::from("new_denom")), 
    });

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("sender88", &vec![]),
        msg,
    )
    .unwrap();

    //Query Config
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Config {},
    )
    .unwrap();
    let config: Config = from_binary(&res).unwrap();

    assert_eq!(
        config,
        Config {
            owner: Addr::unchecked("new_owner"),
            incentive_rate: Decimal::one(), 
            max_incentives: Uint128::new(100),
            unstaking_period: 1,
            minimum_deposit_amount: Uint128::new(10),
            osmosis_proxy: Addr::unchecked("new_op"), 
            positions_contract: Addr::unchecked("new_cdp"), 
            mbrn_denom: String::from("new_denom"), 
        },
    );
}

#[test]
fn capital_ahead_of_deposits() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::zero(),
            deposits: vec![],
        },
        osmosis_proxy: String::from("osmosis_proxy"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        positions_contract: String::from("positions_contract"),
        max_incentives: None,
        minimum_deposit_amount: Uint128::new(5)
    };

    //Instantiating contract
    let info = mock_info("sender88", &vec![]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
   

    //Successful Deposit by user 1
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("user1", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //Successful Deposit by user 2
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("user2", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    //2nd Deposit by user 1
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("user1", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info, deposit_msg).unwrap();

    //2nd Deposit by user 2
    let deposit_msg = ExecuteMsg::Deposit { user: None };
    let info = mock_info("user2", &coins(11, "credit"));
    let _res = execute(deps.as_mut(), mock_env(), info.clone(), deposit_msg).unwrap();

    let query_msg = QueryMsg::CapitalAheadOfDeposit { user: String::from("user1") };
    let res = query(
        deps.as_ref(),
        mock_env(),
        query_msg,
    )
    .unwrap();

    let resp: Vec<DepositPositionResponse> = from_binary(&res).unwrap();

    assert_eq!(
        resp,
        vec![
            DepositPositionResponse { 
                deposit: Deposit { 
                    user: Addr::unchecked("user1"), 
                    amount: Decimal::percent(11_00), 
                    deposit_time: mock_env().block.time.seconds(), 
                    last_accrued: mock_env().block.time.seconds(), 
                    unstake_time: None 
                }, 
                capital_ahead: Decimal::zero(),
            },
            DepositPositionResponse { 
                deposit: Deposit { 
                    user: Addr::unchecked("user1"), 
                    amount: Decimal::percent(11_00), 
                    deposit_time: mock_env().block.time.seconds(), 
                    last_accrued: mock_env().block.time.seconds(), 
                    unstake_time: None 
                }, 
                capital_ahead: Decimal::percent(22_00),
            },
        ]
    );

    
    let query_msg = QueryMsg::CapitalAheadOfDeposit { user: String::from("user2") };
    let res = query(
        deps.as_ref(),
        mock_env(),
        query_msg,
    )
    .unwrap();

    let resp: Vec<DepositPositionResponse> = from_binary(&res).unwrap();

    assert_eq!(
        resp,
        vec![
            DepositPositionResponse { 
                deposit: Deposit { 
                    user: Addr::unchecked("user2"), 
                    amount: Decimal::percent(11_00), 
                    deposit_time: mock_env().block.time.seconds(), 
                    last_accrued: mock_env().block.time.seconds(), 
                    unstake_time: None 
                }, 
                capital_ahead: Decimal::percent(11_00),
            },
            DepositPositionResponse { 
                deposit: Deposit { 
                    user: Addr::unchecked("user2"), 
                    amount: Decimal::percent(11_00), 
                    deposit_time: mock_env().block.time.seconds(), 
                    last_accrued: mock_env().block.time.seconds(), 
                    unstake_time: None 
                }, 
                capital_ahead: Decimal::percent(33_00),
            },
        ]
    );
}
