use crate::contract::{execute, instantiate, query};

use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{ coin, coins, from_binary, Decimal, MemoryStorage, OwnedDeps, Uint128};
use membrane::stability_pool::{
    ClaimsResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
};
use membrane::types::{Asset, AssetInfo, AssetPool, LiqAsset};

const ITERATIONS: u32 = 1u32;

#[test]
fn stress_tests() {
    //Submit deposits and distribute for unique users
    simulate_bids_with_2_liq_amounts(ITERATIONS, 1u128, 1000, 1000);
}

fn instantiate_and_whitelist(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) {
    let msg = InstantiateMsg {
        owner: Some("sender88".to_string()),
        asset_pool: AssetPool {
            credit_asset: Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::zero(),
            },
            liq_premium: Decimal::percent(10),
            deposits: vec![],
        },
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
        for i in 0..liq_amount_1 {
            //Bidders
            let deposit_msg = ExecuteMsg::Deposit {
                asset: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                user: None,
            };
            let bid_info = mock_info(&format!("bidder{}", i), &[coin(bid_amount, "credit")]);
            execute(deps.as_mut(), mock_env(), bid_info.clone(), deposit_msg).unwrap();

            total_bids += bid_amount;
        }

        if i % 2 == 0 {
            let liq_amount = Decimal::from_ratio(Uint128::new(liq_amount_1), Uint128::new(1u128));

            // EXECUTE ALL EXCEPT 1uusd
            let liq_msg = ExecuteMsg::Liquidate {
                liq_amount,               
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
                liq_amount,               
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
                distribute_for: Uint128::new(liq_amount_2),
            };

            let mut coin = coins(distribute_amount.u128(), "debit");
            coin.append(&mut coins(distribute_amount.u128(), "2nddebit"));
            let info = mock_info("positions_contract", &coin);

            execute(deps.as_mut(), mock_env(), info.clone(), distribute_msg).unwrap();
        }
    }

    for i in 0..liq_amount_1 {    
        //Query and assert User claimables
        let res = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::UserClaims {
                user: format!("bidder{}", i),
            },
        )
        .unwrap();

        let resp: ClaimsResponse = from_binary(&res).unwrap();
        
        //+- 2
        //which is .0001 tolerance
        if !(resp.claims[0].amount >= total_distributed/Uint128::new(liq_amount_1/2) - Uint128::new(2)
        && resp.claims[0].amount <= total_distributed/Uint128::new(liq_amount_1/2) + Uint128::new(2)) {
            panic!("{}, {}", resp.claims[0].amount, total_distributed/Uint128::new(liq_amount_1/2) )
        }                
        
    }

    //Query position data to make sure leftover is leftover
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::AssetPool { },
    )
    .unwrap();

    let resp: AssetPool = from_binary(&res).unwrap();
    
    assert_eq!(
        resp.credit_asset.amount.to_string(),
        format!("{}", total_bids - total_liquidated)
    );
    
}
