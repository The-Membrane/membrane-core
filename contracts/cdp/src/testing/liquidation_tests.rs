pub mod tests {

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};
    use membrane::cdp::{
        ExecuteMsg, QueryMsg, EditBasket, UpdateConfig, Config, BasketPositionsResponse,
        CollateralInterestResponse, LiquidationStatResponse,
    };
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo, PoolType,
        SupplyCap, Basket, MultiAssetSupplyCap, LPAssetInfo, PoolInfo, DeploymentIntent,
    };
    use membrane::staking::Config as Staking_Config;
    use cosmwasm_std::{
        attr, coin, coins, to_json_binary, Addr, Binary, Coin, Decimal, Empty, Response,
        StdError, StdResult, Uint128, BlockInfo,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    use crate::testing::integration_tests::tests::proper_instantiate;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    fn get_user_addr(app: &App) -> Addr {
        app.api().addr_make(USER)
    }
    fn get_admin_addr(app: &App) -> Addr {
        app.api().addr_make(ADMIN)
    }

    mod cdp {
        use super::*;

        #[test]
        fn liquidate() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                },
                SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "2nddebit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    },
                    Coin {
                        denom: "2nddebit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(98425000000));
            assert_eq!(res[0].positions[0].collateral_assets[1].asset.amount, Uint128::new(98425000000));

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(12_500_000, "2nddebit"), coin(12_500_000, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(312_500_000, "2nddebit"), coin(312_500_000, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1250_000_000, "2nddebit"), coin(1250_000_000, "debit")]
            );
            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(98425000000)
            );
            assert_eq!(
                res.collateral_supply_caps[1].current_supply,
                Uint128::new(98425000000)
            );
            // Assert liquidation stats saved
            let stats: Vec<LiquidationStatResponse> = app
                .wrap()
                .query_wasm_smart(
                    cdp_contract.addr(),
                    &QueryMsg::GetLiquidationStats { start_after: None, limit: Some(10) },
                )
                .unwrap();
            assert!(stats.len() >= 1);
            assert_eq!(stats.last().unwrap().position_id, Uint128::new(1));
            assert_eq!(stats.last().unwrap().collateral_assets.len(), 2);
            assert!(stats.last().unwrap().amount_liquidated > Uint128::zero());



            //////LQ Query Errors/// Should all be sold using the router//
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                added_cAsset: None,
                take_revenue: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            // A full liquidation + fees leaves nothing
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(0));

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22_222222, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit"), coin(444_444444, "debit")]
            );

            //Assert LQ wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );
            //Assert asset tally is working
            let query_msg = QueryMsg::GetBasket { };
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.collateral_supply_caps[0].current_supply,
                Uint128::new(0)
            );

        }
        #[test]
        fn liquidate_LPs() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make("bigger_bank").to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(97405_616667055555555986));
            //2777 credit liquidated at $1
            //lp_denom is worth $2

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13_888888888888888875, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000000000, "2nddebit"), coin(100000000000, "debit"), coin(416_666_666_666_666_666_250, "lp_denom")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1277_777_777_388_888_888_889, "lp_denom")]
            );

            /////////SP Errors////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, false, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract: SP Errors which cancels liq
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make("bigger_bank").to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Send CDP the LP pool assets to mimic a withdrawal
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //////LQ Errors///
            /// The query erroring will skip all LQ msgs and use the SP.
            /// In real-time if the query doesn't error but the execution does, the liquidation msg will fail.
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make("bigger_bank").to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(98041_666667094444444875));

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(13_888888888888888875, "lp_denom")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit"), coin(100000_000000, "debit"), coin(416_666666666666666250, "lp_denom")]
            );

            //Assert LQ wasn't sent any due to the Error
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );

            //////Both Errors/////
            ///
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(true, true, false, false);

            //Add LP pool assets first: Base
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();
            //Add LP pool assets first: Quote
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(60),
                    max_LTV: Decimal::percent(80),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            //Add LQ
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(40),
                    max_LTV: Decimal::percent(60),
                    pool_info: Some(PoolInfo {
                        pool_id: 99u64,
                        asset_infos: vec![
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("base"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                            LPAssetInfo {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("quote"),
                                },
                                decimals: 6u64,
                                ratio: Decimal::percent(50),
                            },
                        ],
                    }),
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "base".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "quote".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Call liquidate on CDP contract: Both errored
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make("bigger_bank").to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            //Send CDP the LP pool assets to mimic a withdrawal
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                Addr::unchecked(cdp_contract.clone().addr()),
                &vec![coin(1388_888888, "base"), coin(1388_888888, "quote")],
            )
            .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn liquidate_bignums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("bigger_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make("bigger_bank").to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make("bigger_bank").to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].positions[0].collateral_assets[0].asset.amount,
                Uint128::new(97288888888893)
            );

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(22_222_222_222, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000000000, "2nddebit"), coin(544_444_444_444, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(1_999_999_999_997, "debit")]
            );
        }

        #[test]
        fn liquidate_minimums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Lower debt minimum
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                chain_proxy: None,
                ltv_disco: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                affiliate_fee_max: None,
                debt_minimum: Some(Uint128::new(500u128)),
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Increase Debt to a point where a liquidations:
            //1. Liquidates less than the debt_minimum
            //2. Brings the position below the minimum debt
            //..Which then results in a full liquidation
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(999_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(98741_210001));

            //Assert fees were sent.
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(9_990_000, "debit")]
            );
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit"), coin(199_800000, "debit")]
            );

            //Assert collateral to be liquidated was sent
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(499_000001, "debit")]
            );
        }

        #[test]
        fn liquidate_all_user_repay() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Lower debt minimum
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                chain_proxy: None,
                debt_auction: None,
                staking_contract: None,
                oracle_contract: None,
                ltv_disco: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                affiliate_fee_max: None,
                debt_minimum: Some(Uint128::new(500u128)),
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();


            //Set deployment intent
            let msg = ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    position_id: Uint128::from(1u128),
                    destination: "contract10".to_string(),
                    user: app.api().addr_make(USER).to_string(),
                    ltv_to_mint: Decimal::percent(1),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Send cdp contract 1k CDT
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                Addr::unchecked(cdp_contract.addr().to_string()),
                &vec![coin(1000_000_000, "credit_fulldenom")],
            )
            .unwrap();
            //Fulfill intents
            let msg = ExecuteMsg::FulfillIntents {
                users: vec![app.api().addr_make(USER).to_string()]
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Pre-fee balance
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit")]
            );

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(99895_000000));

            //Assert fees were sent.
            //Liq fee of 1% on a 500 CDT repayment
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(5_000_000, "debit")]
            );
            //caller fee
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit"), coin(100_000_000, "debit")]
            );
            //None sent to LQ bc user repaid themselves
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![]
            );
        }
        #[test]
        fn liquidate_user_repay_2_venues() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Lower debt minimum
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                chain_proxy: None,
                debt_auction: None,
                ltv_disco: None,
                staking_contract: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                affiliate_fee_max: None,
                debt_minimum: Some(Uint128::new(500u128)),
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();


            //Set deployment intent
            let msg = ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    position_id: Uint128::from(1u128),
                    destination: "contract10".to_string(),
                    user: app.api().addr_make(USER).to_string(),
                    ltv_to_mint: Decimal::percent(10),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

             //Set deployment intent to venue 2
             let msg = ExecuteMsg::SetUserIntents {
                deployment_intent: DeploymentIntent {
                    position_id: Uint128::from(1u128),
                    destination: "contract11".to_string(),
                    user: app.api().addr_make(USER).to_string(),
                    ltv_to_mint: Decimal::percent(10),
                }
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Send cdp contract 20k CDT
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                Addr::unchecked(cdp_contract.addr().to_string()),
                &vec![coin(20000_000_000, "credit_fulldenom")],
            )
            .unwrap();
            //Fulfill intents
            let msg = ExecuteMsg::FulfillIntents {
                users: vec![app.api().addr_make(USER).to_string()]
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Pre-fee balance
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit")]
            );

            //Call liquidate on CDP contract
            let msg = ExecuteMsg::Liquidate {
                position_id: Uint128::new(1u128),
                position_owner: app.api().addr_make(USER).to_string(),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(
                    UserInfo {
                        position_id: Uint128::new(1),
                        position_owner: app.api().addr_make(USER).to_string(),
                    }
                ),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res[0].positions[0].collateral_assets[0].asset.amount, Uint128::new(91022_444669));

            ///User pays 2k
            ///  LQ pays 6.8k, 88 liq fee, 1.7k caller fee

            //Assert fees were sent.
            //Liq fee of 1% of 8888 liq amount
            assert_eq!(
                app.wrap()
                    .query_all_balances(staking_contract.clone())
                    .unwrap(),
                vec![coin(88_888_888, "debit")]
            );
            //caller fee
            assert_eq!(
                app.wrap().query_all_balances(USER).unwrap(),
                vec![coin(100000_000000, "2nddebit"), coin(1777777777, "debit")]
            );
            //Excess sent to LQ but not all bc user repaid themselves as well
            assert_eq!(
                app.wrap().query_all_balances(lq_contract.addr()).unwrap(),
                vec![coin(6888_888666, "debit")]
            );
        }

    }
}
