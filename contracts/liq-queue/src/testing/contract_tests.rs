use crate::contract::{execute, instantiate, query};
use crate::ContractError;

use membrane::liq_queue::{
    BidResponse, Config, ExecuteMsg, InstantiateMsg, QueryMsg, QueueResponse, ClaimsResponse, SlotResponse,
};
use membrane::math::{Decimal256, Uint256};
use membrane::cdp::ExecuteMsg as CDP_ExecuteMsg;
use membrane::types::{AssetInfo, BidInput};
use membrane::oracle::PriceResponse;

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{
    attr, from_binary, to_binary, BankMsg, Coin, CosmosMsg, Decimal, StdError, SubMsg, Uint128,
    WasmMsg, Addr,
};

#[test]
fn proper_initialization() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("addr0000", &[]);

    // we can just call .unwrap() to assert this was a success
    let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // it worked, let's query the state
    let value: Config =
        from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(
        value,
        Config {
            owner: Addr::unchecked("addr0000"),
            positions_contract: Addr::unchecked("positions_contract"),
            waiting_period: 60u64,
            added_assets: Some(vec![]),
            bid_asset: AssetInfo::NativeToken {
                denom: String::from("cdt"),
            },
            minimum_bid: Uint128::zero(),
            maximum_waiting_bids: 100u64,
        }
    );
}

#[test]
fn update_config() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("addr0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    // update owner
    let msg = ExecuteMsg::UpdateConfig {
        owner: Some("owner0001".to_string()),
        waiting_period: None,
        minimum_bid: None,
        maximum_waiting_bids: None,
    };

    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Owner doesn't update until accepted
    let value: Config =
        from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(
        value,
        Config {
            owner: Addr::unchecked("addr0000"),
            positions_contract: Addr::unchecked("positions_contract"),
            waiting_period: 60u64,
            added_assets: Some(vec![]),
            bid_asset: AssetInfo::NativeToken {
                denom: String::from("cdt"),
            },
            minimum_bid: Uint128::zero(),
            maximum_waiting_bids: 100u64,
        }
    );

    // Update left items
    let info = mock_info("addr0000", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        owner: None,
        waiting_period: Some(100u64),
        minimum_bid: Some(Uint128::one()),
        maximum_waiting_bids: Some(10),
    };

    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // it worked, let's query the state
    let value: Config =
        from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(
        value,
        Config {
            owner: Addr::unchecked("addr0000"),
            positions_contract: Addr::unchecked("positions_contract"),
            waiting_period: 100u64,
            added_assets: Some(vec![]),
            bid_asset: AssetInfo::NativeToken {
                denom: String::from("cdt"),
            },            
            minimum_bid: Uint128::one(),
            maximum_waiting_bids: 10u64,
        }
    );

    // Unauthorized err
    let info = mock_info("owner0000", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        owner: Some("addr0000".to_string()),
        waiting_period: Some(60u64),
        minimum_bid: None,
        maximum_waiting_bids: None,
    };

    let res = execute(deps.as_mut(), mock_env(), info, msg);

    match res {
        Err(ContractError::Unauthorized {}) => {}
        Err(_) => {
            panic!("{}", res.err().unwrap().to_string())
        }
        _ => panic!("Msg sender is unauthorized to make this change"),
    }

     // Accept ownership transfer
     let info = mock_info("owner0001", &[]);
     let msg = ExecuteMsg::UpdateConfig {
         owner: None,
         waiting_period: None,
         minimum_bid: None,
         maximum_waiting_bids: None,
     };
 
     let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
     assert_eq!(0, res.messages.len());
 
     // it worked, let's query the state
     let value: Config =
         from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
     assert_eq!(
         value,
         Config {
             owner: Addr::unchecked("owner0001"),
             positions_contract: Addr::unchecked("positions_contract"),
             waiting_period: 100u64,
             added_assets: Some(vec![]),
             bid_asset: AssetInfo::NativeToken {
                 denom: String::from("cdt"),
             },            
             minimum_bid: Uint128::one(),
             maximum_waiting_bids: 10u64,
         }
     );
}

#[test]
fn submit_bid() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::new(2),
        maximum_waiting_bids: 0u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000u128),
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Invalid bid_for
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "reverse_osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };

    let info = mock_info("addr0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::InvalidAsset {});

    //No Assets sent w/ bid
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };

    let info = mock_info("addr0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "No asset provided, only bid asset allowed".to_string()
        })
    );

    //Invalid Bid Asset sent
    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "osmo".to_string(),
            amount: Uint128::from(1000000u128),
        }],
    );
    let err = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Invalid asset provided, only bid asset allowed".to_string()
        })
    );

    //Invalid Bid Asset sent alongside valid asset
    let info = mock_info(
        "addr0000",
        &[
            Coin {
                denom: "cdt".to_string(),
                amount: Uint128::from(1000000u128),
            },
            Coin {
                denom: "uluna".to_string(),
                amount: Uint128::from(1000000u128),
            },
        ],
    );
    let err = execute(deps.as_mut(), mock_env(), info, msg.clone()).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Invalid asset provided, only bid asset allowed".to_string()
        })
    );

    //Invalid Bid amount
    let invalid_msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 1u8,
        },
        bid_owner: None,
    };

    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1u128),
        }],
    );

    let err = execute(deps.as_mut(), mock_env(), info.clone(), invalid_msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid amount too small, minimum is 2".to_string()
        })
    );

    //Invalid Premium
    let invalid_msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 15u8,
        },
        bid_owner: None,
    };

    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1000000u128),
        }],
    );

    let err = execute(deps.as_mut(), mock_env(), info.clone(), invalid_msg).unwrap_err();
    assert_eq!(err, ContractError::InvalidPremium {});

    // AddQueue w/ no bid threshold, i.e. bids go straight to waiting
    //Used to test waiting bid cap
    let queue_msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "not_osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(0u128),
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, queue_msg).unwrap();
    let waiting_msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "not_osmo".to_string(),
            },
            liq_premium: 1u8,
        },
        bid_owner: None,
    };

    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1_000_000u128),
        }],
    );
    //Error during bid for the queue with no threshold
    let err = execute(deps.as_mut(), mock_env(), info.clone(), waiting_msg).unwrap_err();
    assert_eq!(err, ContractError::TooManyWaitingBids { max_waiting_bids: 0 });

    //Successful Bid for the queue with a threshold
    let env = mock_env();
    let wait_end = env.block.time.plus_seconds(60u64);
    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1_000_001u128),
        }],
    );
    execute(deps.as_mut(), env, info, msg.clone()).unwrap();

   
    let bid_response: BidResponse = from_binary(
        &query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Bid {
                bid_for: AssetInfo::NativeToken {
                    denom: "osmo".to_string(),
                },
                bid_id: Uint128::from(1u128),
            },
        )
        .unwrap(),
    )
    .unwrap();

    //This should have 1_000_001 bc the remaining would've been below the minimum bid amount
    assert_eq!(
        bid_response,
        BidResponse {
            id: Uint128::from(1u128),
            user: "addr0000".to_string(),
            amount: Uint256::from(1000001u128),
            liq_premium: 10u8,
            product_snapshot: Decimal256::one(),
            sum_snapshot: Decimal256::zero(),
            pending_liquidated_collateral: Uint256::zero(),
            wait_end: None,
            epoch_snapshot: Uint128::zero(),
            scale_snapshot: Uint128::zero(),
        }
    );

    //Add new queue
    let queue_msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo_again".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000u128),
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, queue_msg).unwrap();

    //Change config to allow 1 waiting bid
    let config_msg = ExecuteMsg::UpdateConfig {
        maximum_waiting_bids: Some(1),
        owner: None,
        waiting_period: None,
        minimum_bid: None,
        
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, config_msg).unwrap();

    //Successful Bid that should create 1 active and 1 waiting bid at the minimum bid amount
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo_again".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let env = mock_env();
    let wait_end = env.block.time.plus_seconds(60u64);
    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1_000_002u128),
        }],
    );
    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    let bid_response: BidResponse = from_binary(
        &query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Bid {
                bid_for: AssetInfo::NativeToken {
                    denom: "osmo_again".to_string(),
                },
                bid_id: Uint128::from(1u128),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        bid_response,
        BidResponse {
            id: Uint128::from(1u128),
            user: "addr0000".to_string(),
            amount: Uint256::from(1_000_000u128),
            liq_premium: 10u8,
            product_snapshot: Decimal256::one(),
            sum_snapshot: Decimal256::zero(),
            pending_liquidated_collateral: Uint256::zero(),
            wait_end: None,
            epoch_snapshot: Uint128::zero(),
            scale_snapshot: Uint128::zero(),
        }
    );

    let bid_response: BidResponse = from_binary(
        &query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Bid {
                bid_for: AssetInfo::NativeToken {
                    denom: "osmo_again".to_string(),
                },
                bid_id: Uint128::from(2u128),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        bid_response,
        BidResponse {
            id: Uint128::from(2u128),
            user: "addr0000".to_string(),
            amount: Uint256::from(2u128),
            liq_premium: 10u8,
            product_snapshot: Decimal256::one(),
            sum_snapshot: Decimal256::zero(),
            pending_liquidated_collateral: Uint256::zero(),
            wait_end: Some(1571797479),
            epoch_snapshot: Uint128::zero(),
            scale_snapshot: Uint128::zero(),
        }
    );
}

#[test]
fn retract_bid() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::new(2),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let info = mock_info(
        "addr0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1000000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env, info, msg).unwrap();

    //Bid not found
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::new(0u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let info = mock_info("addr0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );

    //Withdrawal too small
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::new(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: Some(Uint256::from(999999u128)),
    };
    let info = mock_info("addr0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::InvalidWithdrawal {  }
    );

    //Successful RetractBid
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::new(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let info = mock_info("addr0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: "addr0000".to_string(),
            amount: vec![Coin {
                denom: "cdt".to_string(),
                amount: Uint128::from(1000000u128),
            }]
        }))]
    );

    //Bid not found after retracting
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::new(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let info = mock_info("addr0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );
}

#[test]
fn execute_bid() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    let info = mock_info("owner0000", &[]);
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 1u8,
        },
        bid_owner: None,
    };
    let info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(500_000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env, info, msg).unwrap();
    /////////////////////

    // required_stable 495,000
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(500_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };

    // unauthorized attempt
    let unauth_info = mock_info("asset0000", &[]); // only owner or positions can execute
    let env = mock_env();
    let err = execute(
        deps.as_mut(),
        env.clone(),
        unauth_info,
        liq_msg.clone(),
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {},);
    let info = mock_info(
        "positions_contract",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(500_000u128),
        }],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), liq_msg.clone()).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "positions_contract".to_string(),
            msg: to_binary(&CDP_ExecuteMsg::Repay {
                position_id: Uint128::new(1u128),
                position_owner: Some("owner01".to_string()),
                send_excess_to: None,
            })
            .unwrap(),
            funds: vec![Coin {
                denom: "cdt".to_string(),
                amount: Uint128::from(495000u128),
            }],
        }))]
    );

    let err = execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap_err();
    assert_eq!(err, ContractError::InsufficientBids {});

    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(500_000_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };

    let res = execute(deps.as_mut(), env, info, liq_msg).unwrap_err();
    assert_eq!(res, ContractError::InsufficientBids {});
}

#[test]
fn claim_liquidations() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };

    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 1u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1_000_000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(5000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();
    /////////
    
    let query_msg = QueryMsg::UserClaims { user: String::from("owner0000") };
    let claims: Vec<ClaimsResponse> = from_binary(&query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap()).unwrap();
    assert_eq!(
        claims,
        vec![
            ClaimsResponse { 
                bid_for: String::from("osmo"), 
                pending_liquidated_collateral: Uint256::from(5000u128),
            },
        ],
    );

    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: Some(vec![Uint128::new(1u128)]),
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "5000"),
        ]
    );
}

#[test]
fn update_queue() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 100u64,
    };

    let info = mock_info("addr0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    //Unauthorized
    let unauth_info = mock_info("owner0000", &[]);
    let err = execute(deps.as_mut(), mock_env(), unauth_info, msg.clone()).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});

    //Success
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let query_msg = QueryMsg::Queue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
    };

    let queue_response: QueueResponse =
        from_binary(&query(deps.as_ref(), mock_env(), query_msg.clone()).unwrap()).unwrap();

    assert_eq!(queue_response.max_premium, Uint128::new(10u128));
    assert_eq!(
        queue_response.bid_threshold,
        Uint256::from(1_000_000_000u128)
    );

    //Successful update
    let msg = ExecuteMsg::UpdateQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Some(Uint128::new(20u128)),
        bid_threshold: Some(Uint256::from(5_000_000u128)),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query to Assert
    let queue_response: QueueResponse =
        from_binary(&query(deps.as_ref(), mock_env(), query_msg).unwrap()).unwrap();
    assert_eq!(queue_response.max_premium, Uint128::new(20u128));
    assert_eq!(
        queue_response.bid_threshold,
        Uint256::from(5_000_000u128)
    );

    //Query Slots to Assert increase
    let query_msg = QueryMsg::PremiumSlots { 
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        start_after: None,
        limit: None, 
    };
    let slots_response: Vec<SlotResponse> = from_binary(&query(deps.as_ref(), mock_env(), query_msg).unwrap()).unwrap();
    assert_eq!(slots_response.len(), 21);
    assert_eq!(slots_response[11].liq_premium, "0.11".to_string());
    assert_eq!(slots_response[20].liq_premium, "0.2".to_string());

}
