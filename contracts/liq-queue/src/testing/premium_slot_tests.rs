use crate::contract::{execute, instantiate, query};
use crate::ContractError;

use membrane::liq_queue::{BidResponse, ExecuteMsg, InstantiateMsg, QueryMsg, SlotResponse};
use membrane::math::{Decimal256, Uint256};
use membrane::types::{AssetInfo, BidInput};
use membrane::oracle::PriceResponse;

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{attr, from_binary, Coin, Decimal, StdError, Uint128};

#[test]
fn one_bidder_distribution() {
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

    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "5000"),
        ]
    );

    // Can only withdraw the leftover since 4950 was used to liquidate
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "osmo"),
            attr("bid_id", "1"),
            attr("amount", "995050"),
        ]
    );
}

#[test]
fn two_bidder_distribution() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 1u64,
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
        bid_threshold: Uint256::zero(),
    };

    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Liquidate 4 at $10
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(10u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(4u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info, liq_msg).unwrap();

    ///Submit 2nd bid
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(60u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Liquidate 6 at $20
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(20u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(6u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    //First bidder participated in 2 liquidations
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "7"),
        ]
    );

    // Nothing to wtihdraw
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );

    //2nd bidder participated in 1 liquidations
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("user0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "3"),
        ]
    );

    // Can only withdraw the leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(2u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );
}

#[test]
fn two_bidder_distribution_big_number() {
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
        bid_threshold: Uint256::zero(),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //First bidder bids 10,000
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(10_000_000_000_u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Liquidate 400 at $10
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(10u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(400_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info, liq_msg).unwrap();

    ///Submit 2nd bid for 6000
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(6_000_000_000_u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Liquidate 600 at $20
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(20u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(600_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    //First bidder participated in 2 liquidations
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "700000000"),
        ]
    );

    // Nothing leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );

    //2nd bidder participated in 1 liquidations
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("user0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "300000000"),
        ]
    );

    // Can only withdraw the leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(2u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );
}

#[test]
fn one_user_two_slots() {
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
        bid_threshold: Uint256::from(5_000_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //First bidder bids 100 at 5%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 5u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100_000_000u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

    ///Submit 2nd bid for 100 at 10%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Liquidate 5 at $10
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(10u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(5_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    execute(deps.as_mut(), env.clone(), info, liq_msg).unwrap();

    //Bidder can claim 5
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "5000000"),
        ]
    );

    //Liquidate 10 at $10
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(10u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(10_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    //Bidder can claim 10
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "9999999"), //Rounding favors the system (ie residue)
        ]
    );

    // Nothing leftover from first bid
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );

    // Can only withdraw the leftover from the 10% bid
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(2u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "osmo"),
            attr("bid_id", "2"),
            attr("amount", "59736835"), // 100 ust - 40.263165 = 59.736835 UST
        ]
    );
}

#[test]
fn completely_empty_pool() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 10u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(5_000_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //First bidder bids 1000
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(1_000_000_000_u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Liquidate 20 at $50
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price:  Decimal::from_ratio(50u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(20_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    execute(deps.as_mut(), env.clone(), info, liq_msg).unwrap();

    ///Submit 2nd bid for 2000
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(2_000_000_000_u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    let msg = QueryMsg::Bid {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_id: Uint128::new(2u128),
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: BidResponse = from_binary(&res).unwrap();
    assert!(!resp.product_snapshot.is_zero());
    assert!(resp.epoch_snapshot == Uint128::from(1u128)); // epoch increased

    let msg = QueryMsg::PremiumSlot {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        premium: 0u64,
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: SlotResponse = from_binary(&res).unwrap();
    assert_eq!(resp.sum_snapshot, Decimal256::zero().to_string()); // reseted
    assert_eq!(resp.product_snapshot, Decimal256::one().to_string()); // reseted
    assert_eq!(resp.liq_premium, Decimal256::zero().to_string());
    assert_eq!(
        resp.total_bid_amount,
        Uint256::from(2000000000u128).to_string()
    ); // only 2nd bid
    assert_eq!(resp.current_epoch, Uint128::from(1u128)); // increased epoch
    assert_eq!(resp.current_scale, Uint128::zero());

    //Liquidate 20 at $50
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price:  Decimal::from_ratio(50u128, 1u128),
            decimals: 6u64,
        }, 
        collateral_amount: Uint256::from(20_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    //First bidder can claim from 1st liq
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "20000000"),
        ]
    );

    // Nothing leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::GenericErr {
            msg: "Bid not found".to_string()
        })
    );

    //2nd bidder participated in 1 liquidation as well
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("user0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "20000000"),
        ]
    );
}

#[test]
fn product_truncated_to_zero() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 60u64,
        minimum_bid: Uint128::zero(),
        maximum_waiting_bids: 2u64,
    };

    let info = mock_info("owner0000", &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "gamm/pool/5".to_string(),
        },
        max_premium: Uint128::new(30u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(10_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //force product to zero
    for _ in 0..8 {
        let msg = ExecuteMsg::SubmitBid {
            bid_input: BidInput {
                bid_for: AssetInfo::NativeToken {
                    denom: "gamm/pool/5".to_string(),
                },
                liq_premium: 0u8,
            },
            bid_owner: None,
        };
        let submit_info = mock_info(
            "owner0000",
            &[Coin {
                denom: "cdt".to_string(),
                amount: Uint128::from(1_000_000_000_u128),
            }],
        );

        let env = mock_env();
        execute(deps.as_mut(), env.clone(), submit_info.clone(), msg).unwrap();

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
            collateral_amount: Uint256::from(999_999_995u128), //5 uusd residue //999_999_999
            bid_for: AssetInfo::NativeToken {
                denom: "gamm/pool/5".to_string(),
            },
            position_id: Uint128::new(1u128),
            position_owner: "owner01".to_string(),
        };
        let info = mock_info("positions_contract", &[]);
        execute(deps.as_mut(), env.clone(), info.clone(), liq_msg).unwrap();
    }

    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "gamm/pool/5".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "gamm/pool/5"),
            attr("collateral_amount", "7999999959"), // 999999995 * 8 = 7,999,999,960 missing 1ucol due to rounding and product resolution
        ]
    );

    let resp: SlotResponse = from_binary(
        &query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::PremiumSlot {
                bid_for: AssetInfo::NativeToken {
                    denom: "gamm/pool/5".to_string(),
                },
                premium: 0u64,
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(resp.total_bid_amount, Uint256::from(40u128).to_string()); // 5 * 8 = 40

    //Only last bid is active
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(8u128),
        bid_for: AssetInfo::NativeToken {
            denom: "gamm/pool/5".to_string(),
        },
        amount: None,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "gamm/pool/5"),
            attr("bid_id", "8"),
            attr("amount", "39"), // 5 * 8 = 40 missing 1ucol due to rounding
        ]
    );
}

#[test]
fn two_bidder_distribution_multiple_common_slots() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 1u64,
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
        bid_threshold: Uint256::from(5_000_000_000_000u128),
    };

    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //First bidder submits 100 to 5%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 5u8,
        },
        bid_owner: None,
    };

    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100_000_000u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Submit 2nd bid, 100 in 5%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 5u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100_000_000u128),
        }],
    );
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    //First bidder submits 200 to 10%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };

    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(200_000_000u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Submit 2nd bid, 200 in 10%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 10u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(200_000_000u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Liquidate 32 at $10
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(10u128, 1u128),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(32_000_000u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    // bidders claiming the collaterals
    //  1st: 5 col from the 5% pool, 11 col from the 10% pool
    //  2nd: 5 col from the 5% pool, 11 col from the 10% pool
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "15999999"), //Rounding error
        ]
    );

    //2nd bidder participated in 1 liquidations
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("user0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "16000001"),
        ]
    );

    // Bid #4 leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(4u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "osmo"),
            attr("bid_id", "4"),
            attr("amount", "150736840"),
        ]
    );

    // Bid #3 leftover
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(3u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "osmo"),
            attr("bid_id", "3"),
            attr("amount", "150736839"),
        ]
    );
}

#[test]
fn scalable_reward_distribution_after_multiple_liquidations() {
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
        bid_threshold: Uint256::from(5_000_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Bidder 1 submits 50 to 0%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(50u128),
        }],
    );
    let env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Bidder 2 submits 100 to 0%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100u128),
        }],
    );
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    //Bidder 3 submits 100 to 0%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0001",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Liquidate 100 at $1
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
        collateral_amount: Uint256::from(100u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the secondary bids
    env.block.time = env.block.time.plus_seconds(60u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    //Bidder 1 submits no new bids

    //Bidder 2 submits another 250 to 0%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(250u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Bidder 3 submits another 250 to 0%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 0u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0001",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(250u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //Liquidate 50 at $1
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
        collateral_amount: Uint256::from(50u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the secondary bids
    env.block.time = env.block.time.plus_seconds(120u64);
    execute(deps.as_mut(), env, info, liq_msg).unwrap();

    // #1 Bidder CLAIMS COLLATERALS AND RETRACTS BID
    // 20 from the first liquidations, 2 from the second
    let msg = ExecuteMsg::ClaimLiquidations {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_ids: None,
    };
    let info = mock_info("owner0000", &[]);
    let res = execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "claim_liquidations"),
            attr("collateral_token", "osmo"),
            attr("collateral_amount", "22"),
        ]
    );

    //Rounding errors grant27 instead of 28
    let msg = ExecuteMsg::RetractBid {
        bid_id: Uint128::from(1u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        amount: None,
    };
    let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("method", "retract_bid"),
            attr("bid_for", "osmo"),
            attr("bid_id", "1"),
            attr("amount", "27"),
        ]
    );
}

#[test]
fn not_enough_bid_for_collateral() {
    let mut deps = mock_dependencies();

    let msg = InstantiateMsg {
        owner: None, //Defaults to sender
        positions_contract: String::from("positions_contract"),
        waiting_period: 1u64,
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
        bid_threshold: Uint256::zero(),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //First bidder submits 100 in the 6%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 6u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "owner0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100u128),
        }],
    );
    let mut env = mock_env();
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    //2nd bidder submits 100 in the 6%
    let msg = ExecuteMsg::SubmitBid {
        bid_input: BidInput {
            bid_for: AssetInfo::NativeToken {
                denom: "osmo".to_string(),
            },
            liq_premium: 6u8,
        },
        bid_owner: None,
    };
    let submit_info = mock_info(
        "user0000",
        &[Coin {
            denom: "cdt".to_string(),
            amount: Uint128::from(100u128),
        }],
    );
    execute(deps.as_mut(), env.clone(), submit_info, msg).unwrap();

    ///Try to liquidate 100 at $3
    ///
    // TOTAL COLLATERAL VALUE: 300 UST
    // TOTAL BID POOL AMOUNT: 200 UST
    let liq_msg = ExecuteMsg::Liquidate {
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6u64,
        },
        collateral_price: PriceResponse {
            prices: vec![],
            price: Decimal::from_ratio(3u128, 1u128),
            decimals: 6u64,
        },
        collateral_amount: Uint256::from(100u128),
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        position_id: Uint128::new(1u128),
        position_owner: "owner01".to_string(),
    };
    let info = mock_info("positions_contract", &[]);
    //Increment time to unlock the second bid
    env.block.time = env.block.time.plus_seconds(70u64);
    let err = execute(deps.as_mut(), env, info, liq_msg).unwrap_err();
    assert_eq!(err, ContractError::InsufficientBids {});
}
