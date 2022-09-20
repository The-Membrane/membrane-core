use crate::ContractError;
use crate::contract::{execute, instantiate, query};
//use crate::state::{AssetInfo, BidInput};

//use cw_multi_test::Contract;
//use cw_multi_test::Contract;
use membrane::liq_queue::{InstantiateMsg, QueryMsg, ConfigResponse, ExecuteMsg, BidResponse, QueueResponse, SlotResponse, LiquidatibleResponse};
use membrane::positions::{ExecuteMsg as CDP_ExecuteMsg};
use membrane::types::{ AssetInfo, BidInput, Asset, Bid };
use membrane::math::{ Uint256, Decimal256 };

use cosmwasm_std::testing::{mock_dependencies_with_balance, mock_env, mock_info, mock_dependencies};
use cosmwasm_std::{coins, from_binary, attr, Uint128, Coin, StdError, SubMsg, CosmosMsg, BankMsg, Decimal, WasmMsg, to_binary, Addr};

#[test]
fn partial_one_collateral_one_slot() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        positions_contract: String::from("positions_contract"),
        owner: None, //Defaults to sender
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 999u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!( resp.total_credit_repaid, String::from("999") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 999u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();

}

#[test]
fn partial_one_collateral_one_slot_w_fees() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //10% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1110u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    //panic!( "{}, {}", resp.leftover_collateral, resp.total_credit_repaid);
    assert_eq!( resp.total_credit_repaid, String::from("999") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1110u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();

}

#[test]
fn one_collateral_one_slot() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1000u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!( resp.total_credit_repaid, String::from("1000") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1000u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();

}

fn one_collateral_one_slot_w_fees() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //10% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1111u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    //panic!( "{}, {}", resp.leftover_collateral, resp.total_credit_repaid);
    assert_eq!( resp.total_credit_repaid, String::from("1000") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 1111u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();
}

#[test]
fn two_slot_w_fees() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //0% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    //10% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2000u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    //panic!( "{}, {}", resp.leftover_collateral, resp.total_credit_repaid);
    assert_eq!( resp.total_credit_repaid, String::from("1900") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2000u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();
}

#[test]
fn partial_two_slot_w_fees() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //0% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    //10% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2222u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!( resp.leftover_collateral, String::from("111") );
    assert_eq!( resp.total_credit_repaid, String::from("2000") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2111u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();
}

#[test]
fn partial_two_slot_w_fees_bignums() {
    let mut deps = mock_dependencies( );

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        basket_id: Uint128::new(1u128),
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(100_000_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    //0% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000_000_000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    //10% premium
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdl".to_string(),
            amount: Uint128::from(1000_000_000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible { 
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() }, 
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2222_222_222u128 ),
        credit_info: AssetInfo::NativeToken { denom: "cdl".to_string() },
        credit_price: Decimal::one(),
    };
    let res = query( deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!( resp.leftover_collateral, String::from("111111111") );
    assert_eq!( resp.total_credit_repaid, String::from("2000000000") );
    
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: Decimal::one(),
        collateral_price: Decimal::one(),
        collateral_amount: Uint256::from( 2111_111_111u128 ),
        bid_for: AssetInfo::NativeToken { denom: "osmo".to_string() },
        bid_with: AssetInfo::NativeToken { denom: "cdl".to_string() },
        basket_id: Uint128::new(1u128),
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();
}