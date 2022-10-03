use crate::contract::{execute, instantiate, query};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{coin, coins, from_binary, Decimal, MemoryStorage, OwnedDeps, Uint128};
use membrane::stability_pool::{
    ClaimsResponse, DepositResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
};
use membrane::types::{Asset, AssetInfo, AssetPool, LiqAsset};

const ITERATIONS: u32 = 100u32;

#[test]
fn stress_tests() {
    // submit bids and execute liquidations repeatedly
    // we can alternate larger and smaller executions to decrease the bid_pool product at different rates

    // with very tight liquidations, constatly resetting product
    // 1M USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 1000000000000u128, 49999999999, 49999999990);
    // 10 USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 10000000u128, 499999, 499999);

    // with greater asset price (10k USD per collateral)
    // 1M USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 1_000_000_000_000_u128, 99999999, 99999999);
    // 10,001 USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 10001000000u128, 1000000, 1000000);

    // alternate tight executions
    // 1M USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 1000000000000u128, 19999999999, 19900000000);
    // 100 USD bids
    simulate_bids_with_2_liq_amounts(ITERATIONS, 100000000u128, 1999999, 1900000);

    // 100k USD bids with very tight liquidations
    simulate_bids_with_2_liq_amounts(ITERATIONS, 100000000000u128, 999999999, 999999999);

    // 1M USD bids
    simulate_bids_with_2_liq_amounts(
        ITERATIONS,
        1_000_000_000_000_u128,
        999999999900, // 10 micros of residue
        999999999999, // no residue
    );
}

fn instantiate_and_whitelist(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) {
    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: Some(AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::percent(10),
            deposits: vec![],
        }),
        dex_router: Some(String::from("router_addr")),
        max_spread: Some(Decimal::percent(10)),
        desired_ratio_of_total_credit_supply: None,
        osmosis_proxy: String::from("osmosis_proxy"),
        positions_contract: String::from("positions_contract"),
        mbrn_denom: String::from("mbrn_denom"),
        incentive_rate: None,
        max_incentives: None,
    };

    //Instantiating contract
    let info = mock_info("sender88", &[]);
    let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
}

fn simulate_bids_with_2_liq_amounts(
    iterations: u32,
    bid_amount: u128,
    liq_amount_1: u128,
    liq_amount_2: u128,
) {
    let mut deps = mock_dependencies();
    instantiate_and_whitelist(&mut deps);

    let env = mock_env();
    let info = mock_info("positions_contract", &[]);

    let mut total_liquidated = 0u128;
    let mut total_distributed = Uint128::zero();
    let mut total_bids = 0u128;

    for i in 0..iterations {
        //Bidders
        let deposit_msg = ExecuteMsg::Deposit {
            assets: vec![AssetInfo::NativeToken {
                denom: "credit".to_string(),
            }],
            user: None,
        };
        let bid_info = mock_info("bidder0000", &[coin(bid_amount, "credit")]);
        execute(deps.as_mut(), mock_env(), bid_info.clone(), deposit_msg).unwrap();

        total_bids += bid_amount;

        if i % 2 == 0 {
            let liq_amount = Decimal::from_ratio(Uint128::new(liq_amount_1), Uint128::new(1u128));

            // EXECUTE ALL EXCEPT 1uusd
            let liq_msg = ExecuteMsg::Liquidate {
                credit_asset: LiqAsset {
                    info: AssetInfo::NativeToken {
                        denom: "credit".to_string(),
                    },
                    amount: liq_amount,
                },
            };
            total_liquidated += liq_amount_1;
            
            execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

            //Distribute
            let distribute_amount = Uint128::new(10_000_000u128);
            total_distributed += distribute_amount;

            let distribute_msg = ExecuteMsg::Distribute {
                distribution_assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: distribute_amount,
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: distribute_amount,
                    },
                ],
                distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
                credit_asset: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                distribute_for: Uint128::new(liq_amount_1),
            };

            let mut coin = coins(distribute_amount.u128(), "debit");
            coin.append(&mut coins(distribute_amount.u128(), "2nddebit"));
            let info = mock_info("positions_contract", &coin);

            execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();
        } else {
            let liq_amount = Decimal::from_ratio(Uint128::new(liq_amount_2), Uint128::new(1u128));

            // EXECUTE ALL EXCEPT 1uusd
            let liq_msg = ExecuteMsg::Liquidate {
                credit_asset: LiqAsset {
                    info: AssetInfo::NativeToken {
                        denom: "credit".to_string(),
                    },
                    amount: liq_amount,
                },
            };
            total_liquidated += liq_amount_2;
           
            execute(deps.as_mut(), mock_env(), info.clone(), liq_msg).unwrap();

            //Distribute
            let distribute_amount = Uint128::new(10_000_000u128);
            total_distributed += distribute_amount;

            let distribute_msg = ExecuteMsg::Distribute {
                distribution_assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: distribute_amount,
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: distribute_amount,
                    },
                ],
                distribution_asset_ratios: vec![Decimal::percent(50), Decimal::percent(50)],
                credit_asset: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                distribute_for: Uint128::new(liq_amount_2),
            };

            let mut coin = coins(distribute_amount.u128(), "debit");
            coin.append(&mut coins(distribute_amount.u128(), "2nddebit"));
            let info = mock_info("positions_contract", &coin);

            execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();
        }
    }

    //Query and assert User claimables
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::UserClaims {
            user: "bidder0000".to_string(),
        },
    )
    .unwrap();

    let resp: ClaimsResponse = from_binary(&res).unwrap();

    assert_eq!(
        resp.claims[0].to_string(),
        format!("{} debit", total_distributed)
    );
    assert_eq!(
        resp.claims[1].to_string(),
        format!("{} 2nddebit", total_distributed)
    );

    //Query position data to make sure leftover is leftover
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetDeposits {
            user: "bidder0000".to_string(),
            asset_info: AssetInfo::NativeToken {
                denom: "credit".to_string(),
            },
        },
    )
    .unwrap();

    let resp: DepositResponse = from_binary(&res).unwrap();

    let mut total_deposits = Decimal::zero();
    for deposit in resp.deposits {
        total_deposits += deposit.amount;
    }

    assert_eq!(
        total_deposits.to_string(),
        format!("{}", total_bids - total_liquidated)
    );
}
