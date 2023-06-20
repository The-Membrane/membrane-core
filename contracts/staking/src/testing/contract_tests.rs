use crate::ContractError;
use crate::contract::{execute, instantiate, query};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{
    attr, coin, coins, from_binary, to_binary, Addr, BankMsg, CosmosMsg, Decimal, SubMsg, Uint128,
    WasmMsg,
};

use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, 
    StakedResponse, TotalStakedResponse, StakerResponse, DelegationResponse, RewardsResponse,
};
use membrane::types::{StakeDeposit, StakeDistribution, DelegationInfo, Delegation};

#[test]
fn update_config(){

    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None,
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    
    let msg = ExecuteMsg::UpdateConfig { 
        owner: Some(String::from("new_owner")),
        unstaking_period: Some(2),  
        osmosis_proxy: Some(String::from("new_op")), 
        positions_contract: Some(String::from("new_cdp")), 
        auction_contract: Some("new_auction".to_string()),
        governance_contract: Some("new_gov".to_string()),
        mbrn_denom: Some(String::from("new_denom")), 
        vesting_contract: Some(String::from("new_bv")), 
        incentive_schedule: Some(StakeDistribution { rate: Decimal::one(), duration: 0 }),
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
    
    //No change yet
    assert_eq!(
        config,
        Config {
            owner: Addr::unchecked("sender88"),
            unstaking_period: 2,  
            osmosis_proxy: Some( Addr::unchecked("new_op")), 
            positions_contract: Some( Addr::unchecked("new_cdp")), 
            auction_contract: Some(Addr::unchecked("new_auction")),
            governance_contract: Some(Addr::unchecked("new_gov")),
            mbrn_denom: String::from("new_denom"), 
            vesting_contract: Some( Addr::unchecked("new_bv")),             
            incentive_schedule: StakeDistribution { rate: Decimal::percent(100), duration: 0 },
            fee_wait_period: 1, 
            
        },
    );
    //Previous owner can still update bc the ownership hasn't transferred yet
    let msg = ExecuteMsg::UpdateConfig { 
        owner: None,
        unstaking_period: None,
        osmosis_proxy: None,
        positions_contract: None,
        auction_contract: None,
        governance_contract: None,
        mbrn_denom: None,
        vesting_contract: None,
        incentive_schedule: None,
        fee_wait_period: Some(0),
    };

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("sender88", &vec![]),
        msg,
    )
    .unwrap();

    //New owner calls update_config
    let msg = ExecuteMsg::UpdateConfig { 
        owner: None,
        unstaking_period: None,
        osmosis_proxy: None,
        positions_contract: None,
        auction_contract: None,
        governance_contract: None,
        mbrn_denom: None,
        vesting_contract: None,
        incentive_schedule: None,
        fee_wait_period: None,
    };

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info("new_owner", &vec![]),
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

    //Assert change after new_owner calls update_config
    assert_eq!(
        config,
        Config {
            owner: Addr::unchecked("new_owner"),
            unstaking_period: 2,  
            osmosis_proxy: Some( Addr::unchecked("new_op")), 
            positions_contract: Some( Addr::unchecked("new_cdp")), 
            auction_contract: Some(Addr::unchecked("new_auction")),
            governance_contract: Some(Addr::unchecked("new_gov")),
            mbrn_denom: String::from("new_denom"), 
            vesting_contract: Some( Addr::unchecked("new_bv")),             
            incentive_schedule: StakeDistribution { rate: Decimal::percent(100), duration: 0 },
            fee_wait_period: 0, 
            
        },
    );
}

#[test]
fn stake() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
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

    //Successful Stake from vesting contract
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("vesting_contract", &[coin(11, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("vesting_contract")),
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
                staker: Addr::unchecked("vesting_contract"),
                amount: Uint128::new(11u128),
                stake_time: mock_env().block.time.seconds(),
                unstake_start_time: None,
            },
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();

    assert_eq!(resp.total_not_including_vested, Uint128::new(10));
    assert_eq!(resp.vested_total,  Uint128::new(11));

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
                ( Uint128::new(10), mock_env().block.time.seconds() )
            ],
        }
    );
}

#[test]
fn delegate() {
    //Instantiate test
    let mut deps = mock_dependencies();

    //Instantiate contract
    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Stake MBRN: sender88
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(10, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Stake MBRN: placeholder99
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("placeholder99", &[coin(10, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Delegate MBRN: Error can't delegate to self
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("sender88")), 
        mbrn_amount: None, 
        delegate: Some(true), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: \"Delegate cannot be the user\"".to_string()
    );   

    //Delegate MBRN: success
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: None,
        delegate: Some(true), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query and Assert Delegations
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(resp.len(), 2);
    assert_eq!(
        resp[1].delegation_info,
        DelegationInfo {
            delegated: vec![],
            delegated_to: vec![
                Delegation {
                    delegator: Addr::unchecked("governator_addr"),
                    amount: Uint128::new(10u128),
                    fluidity: false,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[0].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(10u128),
                    fluidity: false,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );

    //Delegate MBRN: success from placeholder99
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("too_many_addr")), 
        mbrn_amount: None,
        delegate: Some(true), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("placeholder99", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Undelegate: Error
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: None,
        delegate: Some(false), 
        fluid: Some(true),
        commission: None,
    };
    let info = mock_info("placeholder99", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err.to_string(), String::from("Custom Error val: \"Delegator not found in delegated's delegated\""));

    //Undelegate: Error
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("non_governator")), 
        mbrn_amount: Some(Uint128::new(6)), 
        delegate: Some(false), 
        fluid: Some(true),
        commission: Some(Decimal::one()),
    };
    let info = mock_info("sender88", &[]);
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err.to_string(), String::from("membrane::types::DelegationInfo not found"));

    //Undelegate partially, change commission & fluidity
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: Some(Uint128::new(6)), 
        delegate: Some(false), 
        fluid: Some(true),
        commission: Some(Decimal::one()),
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query and Assert Delegations
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(resp.len(), 4);
    assert_eq!(
        resp[2].delegation_info,
        DelegationInfo {
            delegated: vec![],
            delegated_to: vec![
                Delegation {
                    delegator: Addr::unchecked("governator_addr"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            commission: Decimal::one(),
        }        
    );
    assert_eq!(
        resp[0].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );

    //Undelegate fully 
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: None, //this will be more than 4 but it should work anyway
        delegate: Some(false), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query and Assert Delegations were deleted from state
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(resp.len(), 2); //4 -> 2 
}

#[test]
fn commissions() {
    //Instantiate test
    let mut deps = mock_dependencies();

    //Instantiate contract
    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Stake MBRN: sender88
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(1000000, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Delegate MBRN: success
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: Some(Uint128::new(500000)),
        delegate: Some(true), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Delegate sets commission
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: None,
        mbrn_amount: None,
        delegate: None,
        fluid: None, 
        commission: Some(Decimal::percent(10)),
    };
    let info = mock_info("governator_addr", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Skip 30 days
    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(259200 * 10); //30 days

    //Query claimables
    let res = query(deps.as_ref(), env.clone(),
        QueryMsg::UserRewards { user: String::from("sender88") },
    ).unwrap();
    let resp: RewardsResponse = from_binary(&res).unwrap();
    assert_eq!(resp.accrued_interest, Uint128::new(7808u128));

    //Query claimables
    let res = query(deps.as_ref(), env.clone(),
        QueryMsg::UserRewards { user: String::from("governator_addr") },
    ).unwrap();
    let resp: RewardsResponse = from_binary(&res).unwrap();
    assert_eq!(resp.accrued_interest, Uint128::new(410u128));

}



#[test]
fn fluid_delegations() {
    //Instantiate test
    let mut deps = mock_dependencies();

    //Instantiate contract
    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
        fee_wait_period: None,
        mbrn_denom: String::from("mbrn_denom"),
        unstaking_period: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Stake MBRN: sender88
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("sender88", &[coin(10, "mbrn_denom")]);
    let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Delegate MBRN: success
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: None,
        delegate: Some(true), 
        fluid: Some(true), 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Delegate fluid MBRN: success
    let msg = ExecuteMsg::DelegateFluidDelegations { 
        governator_addr: String::from("governator_too_addr"), 
        mbrn_amount: Some(Uint128::new(4)),
    };
    let info = mock_info("governator_addr", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    
    //Query and Assert Delegations
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(resp.len(), 3);
    assert_eq!(
        resp[1].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[2].delegation_info,
        DelegationInfo {
            delegated: vec![],
            delegated_to: vec![
                Delegation {
                    delegator: Addr::unchecked("governator_addr"),
                    amount: Uint128::new(6u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                },
                Delegation {
                    delegator: Addr::unchecked("governator_too_addr"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[0].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(6u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );

    //Remove fluidity from delegations
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_addr")), 
        mbrn_amount: None,
        delegate: None,
        fluid: Some(false), 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query and Assert Delegations
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(resp.len(), 3);
    assert_eq!(
        resp[1].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[2].delegation_info,
        DelegationInfo {
            delegated: vec![],
            delegated_to: vec![
                Delegation {
                    delegator: Addr::unchecked("governator_addr"),
                    amount: Uint128::new(6u128),
                    fluidity: false,
                   time_of_delegation: mock_env().block.time.seconds(),
                },
                Delegation {
                    delegator: Addr::unchecked("governator_too_addr"),
                    amount: Uint128::new(4u128),
                    fluidity: true,
                    time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[0].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(6u128),
                    fluidity: false,
                    time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );

    //Remove fluidity from delegations
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_too_addr")), 
        mbrn_amount: None,
        delegate: None,
        fluid: Some(false), 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Attempt to delegate solid delegations: failure
    //Delegate fluid MBRN: success
    let msg = ExecuteMsg::DelegateFluidDelegations { 
        governator_addr: String::from("governator_too_addr"), 
        mbrn_amount: Some(Uint128::new(4)),
    };
    let info = mock_info("governator_addr", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(res.to_string(), String::from("Custom Error val: \"MBRN amount exceeds total fluid delegatible amount\""));

    //Undelegate MBRN that was fluid delegated: success
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("governator_too_addr")), 
        mbrn_amount: Some(Uint128::new(4)),
        delegate: Some(false), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
}

#[test]
fn unstake() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: Some("owner0000".to_string()),
        positions_contract: Some("positions_contract".to_string()),
        auction_contract: Some("auction_contract".to_string()),
        vesting_contract: Some("vesting_contract".to_string()),
        governance_contract: Some("gov_contract".to_string()),
        osmosis_proxy: Some("osmosis_proxy".to_string()),
        incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
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

    //Delegate MBRN: success
    let msg = ExecuteMsg::UpdateDelegations { 
        governator_addr: Some(String::from("unstaking_barrier")), 
        mbrn_amount: None,
        delegate: Some(true), 
        fluid: None, 
        commission: None,
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Successful Stake from vesting contract
    let msg = ExecuteMsg::Stake { user: None };
    let info = mock_info("vesting_contract", &[coin(11, "mbrn_denom")]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "stake"),
            attr("staker", String::from("vesting_contract")),
            attr("amount", String::from("11")),
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();
    assert_eq!(resp.total_not_including_vested, Uint128::new(10000000));
    assert_eq!(resp.vested_total, Uint128::new(11));

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

    //Skip 3 days
    let mut env = mock_env();
    env.block.time = env.block.time.plus_seconds(259200); //3 days

    //Successful Restake to reset the deposits
    let msg = ExecuteMsg::Restake {
        mbrn_amount: Uint128::new(10_000_000u128),
    };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "restake"),
            attr("restake_amount", String::from("10000000")),
        ]
    );
    //Assert that restake claims work
    assert_eq!(res.messages, vec![        
        SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: String::from("osmosis_proxy"),
            funds: vec![],
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
                denom: String::from("mbrn_denom"),
                amount: Uint128::new(8_219u128), //3 days of rewards for 10_000_000
                mint_to_address: String::from("sender88")
            })
            .unwrap()
        }))
    ]);

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
        
    env.block.time = env.block.time.plus_seconds(259200 *2); //6 days

    //Successful partial unstake w/o withdrawals to assert Restake
    let msg = ExecuteMsg::Unstake { mbrn_amount: Some(Uint128::new(5000001)) };
    let info = mock_info("sender88", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("sender88")),
            attr("unstake_amount", String::from("0")),
        ]
    );
    //Bc its a normal staker, they should have accrued interest
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(16_438u128), //6 days of rewards for 10_000_000
                    mint_to_address: String::from("sender88")
                })
                .unwrap()
            }))
        ]
    );

    //Successful Unstake from vesting contract w/o withdrawals
    let msg = ExecuteMsg::Unstake {
        mbrn_amount: Some(Uint128::new(5u128)),
    };
    let info = mock_info("vesting_contract", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("vesting_contract")),
            attr("unstake_amount", String::from("0")),
        ]
    );
    
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
            attr("unstake_amount", String::from("5000001")),
        ]
    );
    //Bc its a normal staker, they should have accrued interest as well
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                to_address: String::from("sender88"),
                amount: coins(5_000_001, "mbrn_denom"),
            })),
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: String::from("osmosis_proxy"),
                funds: vec![],
                msg: to_binary(&OsmoExecuteMsg::MintTokens {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(8_218u128), //This being 8218 asserts that the returning deposit.stake_time got sync'd correctly with the withdrawn Deposit
                    mint_to_address: String::from("sender88")
                })
                .unwrap()
            }))
        ]
    );

    //Query and Assert Delegations were updated by the unstake
    let res = query(deps.as_ref(), mock_env(),
        QueryMsg::Delegations {
            user: None,
            limit: None,
            start_after: None,
        },
    ).unwrap();
    let resp: Vec<DelegationResponse> = from_binary(&res).unwrap();
    assert_eq!(
        resp[0].delegation_info,
        DelegationInfo {
            delegated: vec![],
            delegated_to: vec![
                Delegation {
                    delegator: Addr::unchecked("unstaking_barrier"),
                    amount: Uint128::new(4999999u128),
                    fluidity: false,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            commission: Decimal::zero(),
        }        
    );
    assert_eq!(
        resp[1].delegation_info,
        DelegationInfo {
            delegated: vec![
                Delegation {
                    delegator: Addr::unchecked("sender88"),
                    amount: Uint128::new(4999999u128),
                    fluidity: false,
                   time_of_delegation: mock_env().block.time.seconds(),
                }
            ],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }        
    );

    //Successful Unstake from vesting contract w/ withdrawals after unstaking period
    let msg = ExecuteMsg::Unstake {
        mbrn_amount: Some(Uint128::new(5u128)),
    };
    let info = mock_info("vesting_contract", &[]);
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "unstake"),
            attr("staker", String::from("vesting_contract")),
            attr("unstake_amount", String::from("5")),
        ]
    );
    //Only msg is stake withdrawal
    assert_eq!(
        res.messages,
        vec![
            SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
                to_address: String::from("vesting_contract"),
                amount: coins(5, "mbrn_denom"),
            })),
        ]
    );

    //Query and Assert totals
    let res = query(deps.as_ref(), mock_env(), QueryMsg::TotalStaked {}).unwrap();

    let resp: TotalStakedResponse = from_binary(&res).unwrap();

    assert_eq!(resp.total_not_including_vested, Uint128::new(4_999_999));
    assert_eq!(resp.vested_total, Uint128::new(6));
}


// #[test]
// fn claim_rewards() {
//     let mut deps = mock_dependencies();

//     let msg = InstantiateMsg {
//         owner: Some("owner0000".to_string()),
//         dex_router: Some(String::from("router_addr")),
//         max_spread: Some(Decimal::percent(10)),
//         positions_contract: Some("positions_contract".to_string()),
//         auction_contract: Some("auction_contract".to_string()),
//         vesting_contract: None,
//         governance_contract: Some("gov_contract".to_string()),
//         osmosis_proxy: Some("osmosis_proxy".to_string()),
//         incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 1101 }),
//         fee_wait_period: None,
//         mbrn_denom: String::from("mbrn_denom"),
//         unstaking_period: None,
//     };

//     //Instantiating contract
//     let info = mock_info("sender88", &[]);
//     let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

//     //Successful Stake for User 1
//     let msg = ExecuteMsg::Stake { user: None };
//     let info = mock_info("user_1", &[coin(10_000_000, "mbrn_denom")]);
//     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

//     //Successful Stake for User 2
//     let msg = ExecuteMsg::Stake { user: None };
//     let info = mock_info("user_2", &[coin(10_000_000, "mbrn_denom")]);
//     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

//     //Successful Stake for User 3
//     let msg = ExecuteMsg::Stake { user: None };
//     let info = mock_info("user_3", &[coin(10_000_000, "mbrn_denom")]);
//     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

//     //Successful Stake for User 4
//     let msg = ExecuteMsg::Stake { user: None };
//     let info = mock_info("user_4", &[coin(10_000_000, "mbrn_denom")]);
//     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    
//     //Successful DepositFee
//     let msg = ExecuteMsg::DepositFee {};
//     let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
//     let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

//     //Query and Assert no rewards due to waiting period
//     let res = query(
//         deps.as_ref(),
//         mock_env(),
//         QueryMsg::StakerRewards {
//             staker: String::from("sender88"),
//         },
//     )
//     .unwrap();

//     let resp: RewardsResponse = from_binary(&res).unwrap();
//     assert_eq!(resp.claimables, vec![]);

//     //Successful DepositFee after the Staker's waiting period
//     let msg = ExecuteMsg::DepositFee {};
//     let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
//     let mut env = mock_env();
//     env.block.time = env.block.time.plus_seconds(86_400u64 * 3u64);
//     let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

//     //Query and Assert Rewards
//     let res = query(
//         deps.as_ref(),
//         mock_env(),
//         QueryMsg::StakerRewards {
//             staker: String::from("user_1"),
//         },
//     )
//     .unwrap();

//     let resp: RewardsResponse = from_binary(&res).unwrap();
//     assert_eq!(
//         resp.claimables,
//         vec![Asset {
//             info: AssetInfo::NativeToken {
//                 denom: String::from("fee_asset")
//             },
//             amount: Uint128::new(2500000000),
//         }]
//     );

   
//     //No stake Error for ClaimRewards
//     let msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: None,
//         send_to: None,
//         restake: false,
//     };
//     let info = mock_info("not_a_staker", &[coin(10, "mbrn_denom")]);
//     let err = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap_err();
//     assert_eq!(
//         err.to_string(),
//         "Generic error: User has no stake".to_string()
//     );   

//     //Claim As Native
//     let claim_msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: Some(String::from("credit")),
//         send_to: None,
//         restake: false,
//     };
//     let info = mock_info("user_1", &[]);
//     let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
//     assert_eq!(
//         res.messages,
//         vec![
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("router_addr"),
//                 funds: coins(2500000000, "fee_asset"),
//                 msg: to_binary(&RouterExecuteMsg::Swap {
//                     to: SwapToAssetsInput::Single(AssetInfo::NativeToken {
//                         denom: String::from("credit")
//                     }),
//                     max_spread: Some(Decimal::percent(10)),
//                     recipient: Some(String::from("user_1")),
//                     hook_msg: None,
//                 })
//                 .unwrap()
//             })),
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("osmosis_proxy"),
//                 funds: vec![],
//                 msg: to_binary(&OsmoExecuteMsg::MintTokens {
//                     denom: String::from("mbrn_denom"),
//                     amount: Uint128::new(8219u128),
//                     mint_to_address: String::from("user_1")
//                 })
//                 .unwrap()
//             }))
//         ]
//     );

//     //Claim As Native + send_to
//     let claim_msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: Some(String::from("credit")),
//         send_to: Some(String::from("receiver")),
//         restake: false,
//     };
//     let info = mock_info("user_2", &[]);
//     let res = execute(deps.as_mut(), env.clone(), info, claim_msg).unwrap();
//     assert_eq!(
//         res.messages,
//         vec![
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("router_addr"),
//                 funds: coins(2500000000, "fee_asset"),
//                 msg: to_binary(&RouterExecuteMsg::Swap {
//                     to: SwapToAssetsInput::Single(AssetInfo::NativeToken {
//                         denom: String::from("credit")
//                     }),
//                     max_spread: Some(Decimal::percent(10)),
//                     recipient: Some(String::from("receiver")),
//                     hook_msg: None,
//                 })
//                 .unwrap()
//             })),
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("osmosis_proxy"),
//                 funds: vec![],
//                 msg: to_binary(&OsmoExecuteMsg::MintTokens {
//                     denom: String::from("mbrn_denom"),
//                     amount: Uint128::new(8219u128),
//                     mint_to_address: String::from("receiver")
//                 })
//                 .unwrap()
//             }))
//         ]
//     );

//     //Reset Rewards
//     //Successful DepositFee after the Staker's waiting period
//     let msg = ExecuteMsg::DepositFee {};
//     let info = mock_info("positions_contract", &[coin(10_000_000_000, "fee_asset")]);
//     env.block.time = env.block.time.plus_seconds(86_400u64 * 3u64);
//     let _res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

//     //Successful Staker ClaimRewards
//     let msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: None,
//         send_to: None,
//         restake: false,
//     };
//     let info = mock_info("user_1", &[]);

//     env.block.time = env.block.time.plus_seconds(31_536_000); //Seconds in a Year

//     let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
//     assert_eq!(
//         res.messages,
//         vec![
//             SubMsg::new(CosmosMsg::Bank(BankMsg::Send {
//                 to_address: String::from("user_1"),
//                 amount: coins(2500000000, "fee_asset"),
//             })),
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("osmosis_proxy"),
//                 funds: vec![],
//                 msg: to_binary(&OsmoExecuteMsg::MintTokens {
//                     denom: String::from("mbrn_denom"),
//                     amount: Uint128::new(1_008_219u128),
//                     mint_to_address: String::from("user_1")
//                 })
//                 .unwrap()
//             })),
//         ]
//     );

//     //Secondary claim gives nothing
//     let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
//     assert_eq!(res.messages, vec![]);
   
//     //Restake
//     let msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: None,
//         send_to: None,
//         restake: true,
//     };
//     let info = mock_info("user_1", &[]);
//     env.block.time = env.block.time.plus_seconds(31_536_000); //Add a year

//     let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
//     assert_eq!(
//         res.messages,
//         vec![
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("osmosis_proxy"),
//                 funds: vec![],
//                 msg: to_binary(&OsmoExecuteMsg::MintTokens {
//                     denom: String::from("mbrn_denom"),
//                     amount: Uint128::new(1_000_000),
//                     mint_to_address: String::from("cosmos2contract")
//                 })
//                 .unwrap()
//             })),
//             SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//                 contract_addr: String::from("cosmos2contract"),
//                 funds: vec![coin(1_000_000, "mbrn_denom")],
//                 msg: to_binary(&ExecuteMsg::Stake {
//                     user: Some(String::from("user_1")),
//                 })
//                 .unwrap()
//             })),
//         ]
//     );

//     //SendTo
//     //Every year's stake was claimed, that's why it only mints 1_000_000
//     let msg = ExecuteMsg::ClaimRewards {
//         claim_as_native: None,
//         send_to: Some(String::from("receiver")),
//         restake: false,
//     };
//     let info = mock_info("user_1", &[]);
//     env.block.time = env.block.time.plus_seconds(31_536_000);

//     let res = execute(deps.as_mut(), env, info, msg).unwrap();
//     assert_eq!(
//         res.messages,
//         vec![SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
//             contract_addr: String::from("osmosis_proxy"),
//             funds: vec![],
//             msg: to_binary(&OsmoExecuteMsg::MintTokens {
//                 denom: String::from("mbrn_denom"),
//                 amount: Uint128::new(1_000_000u128),
//                 mint_to_address: String::from("receiver")
//             })
//             .unwrap()
//         }))]
//     );
// }
