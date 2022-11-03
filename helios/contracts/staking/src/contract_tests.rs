use crate::contract::{execute, instantiate, query};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{
    attr, coin, coins, from_binary, to_binary, Addr, BankMsg, CosmosMsg, Decimal, SubMsg, Uint128,
    WasmMsg,
};
use cw20::Cw20ReceiveMsg;

use membrane::apollo_router::ExecuteMsg as RouterExecuteMsg;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    Config, Cw20HookMsg, ExecuteMsg, FeeEventsResponse, InstantiateMsg, QueryMsg, RewardsResponse,
    StakedResponse, TotalStakedResponse, StakerResponse,
};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit};


#[test]
fn update_config(){

    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None,
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        positions_contract: Some("positions_contract".to_string()),
        builders_contract: Some("builders_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        staking_rate: Some(Decimal::percent(10)),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    
    let msg = ExecuteMsg::UpdateConfig { 
        owner: Some(String::from("new_owner")),
        unstaking_period: Some(1),  
        osmosis_proxy: Some(String::from("new_op")), 
        positions_contract: Some(String::from("new_cdp")), 
        mbrn_denom: Some(String::from("new_denom")), 
        dex_router: Some(String::from("new_router")), 
        max_spread: Some(Decimal::one()),
        builders_contract: Some(String::from("new_bv")), 
        staking_rate: Some(Decimal::one()),
        fee_wait_period: Some(1),  
    };

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
            unstaking_period:1,  
            osmosis_proxy: Some( Addr::unchecked("new_op")), 
            positions_contract: Some( Addr::unchecked("new_cdp")), 
            mbrn_denom: String::from("new_denom"), 
            dex_router: Some( Addr::unchecked("new_router")), 
            max_spread: Some(Decimal::one()), 
            builders_contract: Some( Addr::unchecked("new_bv")), 
            staking_rate: Decimal::percent(20), //Capped at 20% that's why it isn't 1
            fee_wait_period: 1, 
            
        },
    );
}

#[test]
fn stake() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        positions_contract: Some("positions_contract".to_string()),
        builders_contract: Some("builders_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        staking_rate: Some(Decimal::percent(10)),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Stake non-MBRN asset
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(10, "not-mbrn")]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: \"No valid assets\"".to_string()
    );

    //Successful Stake
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(10, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("sender88")),
            attr("amount", String::from("10")),
        ]
    );

    //Successful Stake from builders contract
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("builders_contract", &[coin(11, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("builders_contract")),
            attr("amount", String::from("11")),
        ]
    );

    //Query and Assert Stakers
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::Staked {
            limit: None,
            start_after: None,
            end_before: None,
            unstaking: false,
        },
    )
    .unwrap();
    let resp: StakedResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp.stakers,
        vec![
            StakeDeposit {
                staker: Addr::unchecked("sender88"),
                amount: Uint128::new(10u128),
                stake_time: mock_env().block.time.seconds(),
                unstake_start_time: None,
            },
            StakeDeposit {
                staker: Addr::unchecked("builders_contract"),
                amount: Uint128::new(11u128),
                stake_time: mock_env().block.time.seconds(),
                unstake_start_time: None,
            },
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();

    assert_eq!(resp.total_not_including_builders, String::from("10"));
    assert_eq!(resp.builders_total, String::from("11"));

    //Query and Assert User stake
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserStake { staker: String::from("sender88") }
    )
    .unwrap();
    let resp: StakerResponse = from_binary(&res).unwrap();
    assert_eq!(resp, 
        StakerResponse { 
            staker: String::from("sender88"),
            total_staked: Uint128::new(10),
            deposit_list: vec![
                ( String::from("10"), mock_env().block.time.seconds().to_string() )
            ],
        }
    );
}

#[test]
fn unstake() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        positions_contract: Some("positions_contract".to_string()),
        builders_contract: Some("builders_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        staking_rate: Some(Decimal::percent(10)),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(10_000_000, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("sender88")),
            attr("amount", String::from("10000000")),
        ]
    );

    //Successful Stake from builders contract
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("builders_contract", &[coin(11, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("builders_contract")),
            attr("amount", String::from("11")),
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();
    assert_eq!(resp.total_not_including_builders, String::from("10000000"));
    assert_eq!(resp.builders_total, String::from("11"));

    //Unstake more than Staked Error
    let msg = ExecuteMsg::Unstake {
        mbrn_amount: Some(Uint128::new(11_000_000u128)),
    };
    let info = mock_info("sender88", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: \"Invalid withdrawal amount\"".to_string()
    );

    //Not a staker Error
    let msg = ExecuteMsg::Unstake { mbrn_amount: None };
    let info = mock_info("not_a_user", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: \"User has no stake\"".to_string()
    );

    //Successful Unstake w/o withdrawals
    let msg = ExecuteMsg::Unstake { mbrn_amount: None };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("sender88")),
            attr("unstake_amount", String::from("0")),
        ]
    );

    //Successful Restake to reset the deposits
    let msg = ExecuteMsg::Restake {
        mbrn_amount: Uint128::new(10_000_000u128),
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "restake"),
            attr("restake_amount", String::from("10000000")),
        ]
    );

    //Successful Unstake w/o withdrawals to assert Restake
    let msg = ExecuteMsg::Unstake { mbrn_amount: None };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("sender88")),
            attr("unstake_amount", String::from("0")),
        ]
    );

    //Successful Unstake from builders contract w/o withdrawals
    let msg = ExecuteMsg::Unstake {
        mbrn_amount: Some(Uint128::new(5u128)),
    };
    let info = mock_info("builders_contract", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("builders_contract")),
            attr("unstake_amount", String::from("0")),
        ]
    );

    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(259200); //3 days

    //Successful Unstake w/ withdrawals after unstaking period
    let msg = ExecuteMsg::Unstake { mbrn_amount: None };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("sender88")),
            attr("unstake_amount", String::from("10000000")),
        ]
    );
    //Bc its a normal staker, they should have accrued interest as well
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                to_address: String::from("sender88"),
                amount: coins(10_000_000, "mbrn_denom"),
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8_219u128),
                    mint_to_address: String::from("sender88")
                })
                .unwrap()
            }))
        ]
    );

    //Successful Unstake from builders contract w/ withdrawals after unstaking period
    let msg = ExecuteMsg::Unstake {
        mbrn_amount: Some(Uint128::new(5u128)),
    };
    let info = mock_info("builders_contract", &[]);
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("builders_contract")),
            attr("unstake_amount", String::from("5")),
        ]
    );
    //Only msg is stake withdrawal
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                to_address: String::from("builders_contract"),
                amount: coins(5, "mbrn_denom"),
            })),
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();

    assert_eq!(resp.total_not_including_builders, String::from("0"));
    assert_eq!(resp.builders_total, String::from("6"));
}

#[test]
fn deposit_fee() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        positions_contract: Some("positions_contract".to_string()),
        builders_contract: Some("builders_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        staking_rate: Some(Decimal::percent(10)),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Unauthorized
    let msg = ExecuteMsg::DepositFee {};
    let info = mock_info("sender88", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized".to_string());

    //Successful DepositFee
    let msg = ExecuteMsg::DepositFee {};
    let info = mock_info("positions_contract", &[coin(10, "fee_asset")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "deposit_fee"),
            attr("fee_assets", String::from("[\"10 fee_asset\"]")),
        ]
    );

    //Successful Cw20 DepositFee
    let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
        sender: String::from("positions_contract"),
        amount: Uint128::new(10),
        msg: to_binary(&Cw20HookMsg::DepositFee {}).unwrap(),
    });

    let info = mock_info("cw20_asset", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "deposit_fee"),
            attr("fee_assets", String::from("[\"10 cw20_asset\"]")),
        ]
    );

    //Query and Assert totals from FeeEvents
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::FeeEvents {
            limit: None,
            start_after: None,
        },
    )
    .unwrap();

    let resp: FeeEventsResponse = from_binary(&res).unwrap();

    assert_eq!(
        resp.fee_events,
        vec![
            FeeEvent {
                time_of_event: mock_env().block.time.seconds(),
                fee: LiqAsset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("fee_asset")
                    },
                    amount: Decimal::percent(1000),
                },
            },
            FeeEvent {
                time_of_event: mock_env().block.time.seconds(),
                fee: LiqAsset {
                    info: AssetInfo::Token {
                        address: Addr::unchecked("cw20_asset")
                    },
                    amount: Decimal::percent(1000),
                },
            },
        ]
    );
}

#[test]
fn claim_rewards() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        positions_contract: Some("positions_contract".to_string()),
        builders_contract: Some("builders_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        staking_rate: Some(Decimal::percent(10)),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake for User 1
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("user_1", &[coin(10_000_000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake for User 2
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("user_2", &[coin(10_000_000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake for User 3
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("user_3", &[coin(10_000_000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake for User 4
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("user_4", &[coin(10_000_000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake from builders contract
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("builders_contract", &[coin(11_000_000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful DepositFee
    let msg = ExecuteMsg::DepositFee {};
    let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query and Assert no rewards due to waiting period
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StakerRewards {
            staker: String::from("sender88"),
        },
    )
    .unwrap();

    let resp: RewardsResponse = from_binary(&res).unwrap();
    assert_eq!(resp.claimables, vec![]);

    //Successful DepositFee after the Staker's waiting period
    let msg = ExecuteMsg::DepositFee {};
    let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(86_400u64 * 3u64);
    let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    //Query and Assert Rewards
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StakerRewards {
            staker: String::from("user_1"),
        },
    )
    .unwrap();

    let resp: RewardsResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp.claimables,
        vec![Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("fee_asset")
            },
            amount: Uint128::new(1_960_784_313u128),
        }]
    );

    //Query and Assert Rewards
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StakerRewards {
            staker: String::from("builders_contract"),
        },
    )
    .unwrap();

    let resp: RewardsResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp.claimables,
        vec![Asset {
            info: AssetInfo::NativeToken {
                denom: String::from("fee_asset")
            },
            amount: Uint128::new(2_156_862_745u128),
        }]
    );

    //No stake Error for ClaimRewards
    let msg = ExecuteMsg::ClaimRewards {
        claim_as_cw20: None,
        claim_as_native: None,
        send_to: None,
        restake: false,
    };
    let info = mock_info("not_a_staker", &[coin(10, "mbrn_denom")]);
    let err = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Generic error: User has no stake".to_string()
    );

    //Error
    let claim_msg = ExecuteMsg::ClaimRewards {
        claim_as_native: Some(String::from("credit")),
        claim_as_cw20: Some(String::from("protocol_token")),
        send_to: None,
        restake: false,
    };
    //Can't claim as two different assets Error
    let err = execute(deps.as_mut(), mock_env(), info, claim_msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        String::from(
            "Custom Error val: \"Can't claim as multiple assets, if not all claimable assets\""
        )
    );

    //Claim As Native
    let claim_msg = ExecuteMsg::ClaimRewards {
        claim_as_native: Some(String::from("credit")),
        claim_as_cw20: None,
        send_to: None,
        restake: false,
    };
    let info = mock_info("user_1", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(1_960_784_313, "fee_asset"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative {
                    to: AssetInfo::NativeToken {
                        denom: String::from("credit")
                    },
                    max_spread: Some(Decimal::percent(10)),
                    recipient: Some(String::from("user_1")),
                    hook_msg: None,
                    split: None,
                })
                .unwrap()
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8219u128),
                    mint_to_address: String::from("user_1")
                })
                .unwrap()
            }))
        ]
    );

    //Claim As Native + send_to
    let claim_msg = ExecuteMsg::ClaimRewards {
        claim_as_native: Some(String::from("credit")),
        claim_as_cw20: None,
        send_to: Some(String::from("receiver")),
        restake: false,
    };
    let info = mock_info("user_2", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(1_960_784_313, "fee_asset"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative {
                    to: AssetInfo::NativeToken {
                        denom: String::from("credit")
                    },
                    max_spread: Some(Decimal::percent(10)),
                    recipient: Some(String::from("receiver")),
                    hook_msg: None,
                    split: None,
                })
                .unwrap()
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8219u128),
                    mint_to_address: String::from("receiver")
                })
                .unwrap()
            }))
        ]
    );

    //Claim As Cw20
    let claim_msg = ExecuteMsg::ClaimRewards {
        claim_as_native: None,
        claim_as_cw20: Some(String::from("credit")),
        send_to: None,
        restake: false,
    };
    let info = mock_info("user_3", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(1_960_784_313, "fee_asset"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative {
                    to: AssetInfo::Token {
                        address: Addr::unchecked("credit")
                    },
                    max_spread: Some(Decimal::percent(10)),
                    recipient: Some(String::from("user_3")),
                    hook_msg: None,
                    split: None,
                })
                .unwrap()
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8219u128),
                    mint_to_address: String::from("user_3")
                })
                .unwrap()
            }))
        ]
    );

    //Claim As Cw20 + send_to
    let claim_msg = ExecuteMsg::ClaimRewards {
        claim_as_native: None,
        claim_as_cw20: Some(String::from("credit")),
        send_to: Some(String::from("receiver")),
        restake: false,
    };
    let info = mock_info("user_4", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("router_addr"),
                funds: coins(1_960_784_313, "fee_asset"),
                msg: to_binary(&RouterExecuteMsg::SwapFromNative {
                    to: AssetInfo::Token {
                        address: Addr::unchecked("credit")
                    },
                    max_spread: Some(Decimal::percent(10)),
                    recipient: Some(String::from("receiver")),
                    hook_msg: None,
                    split: None,
                })
                .unwrap()
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8219u128),
                    mint_to_address: String::from("receiver")
                })
                .unwrap()
            }))
        ]
    );

    //Reset Rewards
    //Successful DepositFee after the Staker's waiting period
    let msg = ExecuteMsg::DepositFee {};
    let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
    env.block.time = env.block.time.plus_seconds(86_400u64 * 3u64);
    let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    //Successful Staker ClaimRewards
    let msg = ExecuteMsg::ClaimRewards {
        claim_as_cw20: None,
        claim_as_native: None,
        send_to: None,
        restake: false,
    };
    let info = mock_info("user_1", &[]);

    env.block.time = env.block.time.plus_seconds(31_536_000); //Seconds in a Year

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                to_address: String::from("user_1"),
                amount: coins(1_960_784_313, "fee_asset"),
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(1_008_219u128),
                    mint_to_address: String::from("user_1")
                })
                .unwrap()
            })),
        ]
    );

    //Secondary claim gives nothing
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(res.messages, vec![]);

    //Builders contract claim is only Fee rewards
    let msg = ExecuteMsg::ClaimRewards {
        claim_as_cw20: None,
        claim_as_native: None,
        send_to: None,
        restake: false,
    };
    let info = mock_info("builders_contract", &[]);

    env.block.time = env.block.time.plus_seconds(31_536_000);

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
            to_address: String::from("builders_contract"),
            amount: coins(4_313_725_490, "fee_asset"),
        })),]
    );

    //Restake
    let msg = ExecuteMsg::ClaimRewards {
        claim_as_cw20: None,
        claim_as_native: None,
        send_to: None,
        restake: true,
    };
    let info = mock_info("user_1", &[]);
    env.block.time = env.block.time.plus_seconds(31_536_000);

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(2_000_000u128),
                    mint_to_address: String::from("cosmos2contract")
                })
                .unwrap()
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("cosmos2contract"),
                funds: vec![coin(2_000_000, "mbrn_denom")],
                msg: to_binary(&ExecuteMsg::Stake {
                    user: Some(String::from("user_1")),
                })
                .unwrap()
            })),
        ]
    );

    //SendTo
    //Every year's stake was claimed, that's why it only mints 1_000_000
    let msg = ExecuteMsg::ClaimRewards {
        claim_as_cw20: None,
        claim_as_native: None,
        send_to: Some(String::from("receiver")),
        restake: false,
    };
    let info = mock_info("user_1", &[]);
    env.block.time = env.block.time.plus_seconds(31_536_000);

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: String::from("osmosis_proxy"),
            funds: vec![],
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
                denom: String::from("mbrn_denom"),
                amount: Uint128::new(1_000_000u128),
                mint_to_address: String::from("receiver")
            })
            .unwrap()
        }))]
    );
}