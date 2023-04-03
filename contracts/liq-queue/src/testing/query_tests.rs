use crate::contract::{execute, instantiate, query};

use membrane::liq_queue::{
    BidResponse, ExecuteMsg, InstantiateMsg, LiquidatibleResponse, QueryMsg, QueueResponse,
    SlotResponse,
};
use membrane::math::{Decimal256, Uint256};
use membrane::types::{AssetInfo, Bid, BidInput, Asset};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_binary, Addr, Coin, Decimal, Uint128};

#[test]
fn query_liquidatible() {
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
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    let msg = QueryMsg::CheckLiquidatible {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        collateral_price: Decimal::percent(100),
        collateral_amount: Uint256::from(10_000u128),
        credit_info: AssetInfo::NativeToken {
            denom: "cdt".to_string(),
        },
        credit_price: Decimal::percent(100),
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: LiquidatibleResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        LiquidatibleResponse {
            leftover_collateral: String::from("0"),
            total_debt_repaid: String::from("9900"),
        }
    );
}

#[test]
fn query_bid() {
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

    //Submit 1 Bid
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
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    //Query Individual Bid
    let msg = QueryMsg::Bid {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        bid_id: Uint128::from(1u128),
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: BidResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        BidResponse {
            user: String::from("owner0000"),
            id: Uint128::new(1u128),
            amount: Uint256::from(1_000_000u128),
            liq_premium: 1u8,
            product_snapshot: Decimal256::one(),
            sum_snapshot: Decimal256::zero(),
            pending_liquidated_collateral: Uint256::zero(),
            wait_end: None,
            epoch_snapshot: Uint128::zero(),
            scale_snapshot: Uint128::zero(),
        }
    );

    //Submit 2nd Bid
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
            amount: Uint128::from(1_000_000u128),
        }],
    );

    let env = mock_env();
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    //Query User Bids
    let msg = QueryMsg::BidsByUser {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        user: String::from("owner0000"),
        limit: None,
        start_after: None,
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: Vec<BidResponse> = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        vec![
            BidResponse {
                user: String::from("owner0000"),
                id: Uint128::new(1u128),
                amount: Uint256::from(1_000_000u128),
                liq_premium: 1u8,
                product_snapshot: Decimal256::one(),
                sum_snapshot: Decimal256::zero(),
                pending_liquidated_collateral: Uint256::zero(),
                wait_end: None,
                epoch_snapshot: Uint128::zero(),
                scale_snapshot: Uint128::zero(),
            },
            BidResponse {
                user: String::from("owner0000"),
                id: Uint128::new(2u128),
                amount: Uint256::from(1_000_000u128),
                liq_premium: 10u8,
                product_snapshot: Decimal256::one(),
                sum_snapshot: Decimal256::zero(),
                pending_liquidated_collateral: Uint256::zero(),
                wait_end: None,
                epoch_snapshot: Uint128::zero(),
                scale_snapshot: Uint128::zero(),
            },
        ]
    );
}

#[test]
fn query_slots_queues() {
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
    execute(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let msg = ExecuteMsg::AddQueue {
        bid_for: AssetInfo::NativeToken {
            denom: "atom".to_string(),
        },
        max_premium: Uint128::new(10u128), //A slot for each premium is created when queue is created
        bid_threshold: Uint256::from(1_000_000_000u128),
    };
    execute(deps.as_mut(), mock_env(), info, msg).unwrap();

    //Query Queues
    let msg = QueryMsg::Queues {
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: Vec<QueueResponse> = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        vec![
            QueueResponse {
                bid_asset: Asset {
                    amount: Uint128::new(0),
                    info: AssetInfo::NativeToken {
                        denom: "cdt".to_string(),
                    },
                },
                max_premium: Uint128::new(10),
                current_bid_id: Uint128::new(1),
                bid_threshold: Uint128::new(1000000000).into(),
            },
            QueueResponse {
                bid_asset: Asset {
                    amount: Uint128::new(0),
                    info: AssetInfo::NativeToken {
                        denom: "cdt".to_string(),
                    },
                },
                max_premium: Uint128::new(10),
                current_bid_id: Uint128::new(1),
                bid_threshold: Uint128::new(1000000000).into(),
            }
        ]
    );

    //Query All Slots
    let msg = QueryMsg::PremiumSlots {
        bid_for: AssetInfo::NativeToken {
            denom: "atom".to_string(),
        },
        start_after: Some(1),
        limit: Some(2),
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: Vec<SlotResponse> = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        vec![
            SlotResponse {
                bids: vec![],
                waiting_bids: vec![],
                liq_premium: Decimal256::percent(2).to_string(),
                sum_snapshot: Uint128::zero().to_string(),
                product_snapshot: Decimal::one().to_string(),
                total_bid_amount: Uint128::zero().to_string(),
                current_epoch: Uint128::zero(),
                current_scale: Uint128::zero(),
                residue_collateral: Uint128::zero().to_string(),
                residue_bid: Uint128::zero().to_string(),
            },
            SlotResponse {
                bids: vec![],
                waiting_bids: vec![],
                liq_premium: Decimal256::percent(3).to_string(),
                sum_snapshot: Uint128::zero().to_string(),
                product_snapshot: Decimal::one().to_string(),
                total_bid_amount: Uint128::zero().to_string(),
                current_epoch: Uint128::zero(),
                current_scale: Uint128::zero(),
                residue_collateral: Uint128::zero().to_string(),
                residue_bid: Uint128::zero().to_string(),
            }
        ]
    );

    //Submit 1 Bid
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
    execute(deps.as_mut(), env, submit_info, msg).unwrap();

    //Query a Slot
    let msg = QueryMsg::PremiumSlot {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
        premium: 1u64,
    };

    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: SlotResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        SlotResponse {
            bids: vec![Bid {
                user: Addr::unchecked("owner0000"),
                id: Uint128::new(1u128),
                amount: Uint256::from(1_000_000u128),
                liq_premium: 1u8,
                product_snapshot: Decimal256::one(),
                sum_snapshot: Decimal256::zero(),
                pending_liquidated_collateral: Uint256::zero(),
                wait_end: None,
                epoch_snapshot: Uint128::zero(),
                scale_snapshot: Uint128::zero(),
            }],
            waiting_bids: vec![],
            liq_premium: Decimal256::percent(1).to_string(),
            sum_snapshot: Uint128::zero().to_string(),
            product_snapshot: Decimal::one().to_string(),
            total_bid_amount: Uint128::new(1_000_000).to_string(),
            current_epoch: Uint128::zero(),
            current_scale: Uint128::zero(),
            residue_collateral: Uint128::zero().to_string(),
            residue_bid: Uint128::zero().to_string(),
        }
    );

    //Query a QueueResponse
    let msg = QueryMsg::Queue {
        bid_for: AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        },
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: QueueResponse = from_binary(&res).unwrap();
    assert_eq!(
        resp,
        QueueResponse {
            bid_asset: Asset {
                amount: Uint128::new(1_000_000),
                info: AssetInfo::NativeToken {
                    denom: "cdt".to_string(),
                },
            },
            max_premium: Uint128::new(10),
            current_bid_id: Uint128::new(2),
            bid_threshold: Uint128::new(1000000000).into(),
        }
    );

    //Query QueueResponse w/ start after
    let msg = QueryMsg::Queues {
        start_after: Some(AssetInfo::NativeToken {
            denom: "osmo".to_string(),
        }),
        limit: None,
    };
    let res = query(deps.as_ref(), mock_env(), msg).unwrap();
    let resp: Vec<QueueResponse> = from_binary(&res).unwrap();
    assert_eq!(
        resp[0],
        QueueResponse {
            bid_asset: Asset {
                amount: Uint128::new(1_000_000),
                info: AssetInfo::NativeToken {
                    denom: "cdt".to_string(),
                },
            },
            max_premium: Uint128::new(10),
            current_bid_id: Uint128::new(2),
            bid_threshold: Uint128::new(1000000000).into(),
        }
    );
    assert_eq!(resp.len().to_string(), String::from("2"));
}
