pub mod tests {
    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};
    use membrane::cdp::{ExecuteMsg, QueryMsg, EditBasket, UpdateConfig, Config, BasketPositionsResponse};
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo, PoolType, SupplyCap, Basket,
    };
    use cosmwasm_std::{
        attr, coin, to_json_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128, BlockInfo,
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
        fn bad_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

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
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Deposit #1
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
                        amount: Uint128::from(10_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("little_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(1_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("little_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2000_000_000u128)),
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
        }

        #[test]
        fn insolvency_checks() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

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
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

            //Deposit #1
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
                        amount: Uint128::from(10_000_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Deposit #2
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("little_bank").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(1_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("little_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt for 1 position
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000_000000u128)),
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
        }

        // #[test]
        fn two_collateral_cdp_LTV_tests() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, true, false, false);

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
                            denom: "double_debit".to_string(),
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
                            denom: "double_debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

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
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000000u128),
                        },
                        Coin {
                            denom: "double_debit".to_string(),
                            amount: Uint128::from(10_000_000000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(4999_000000u128)),
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

            //Query for LTV
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
            //Assert LTVs
            assert_eq!(res[0].positions[0].avg_borrow_LTV.to_string(), String::from("0.55"));
            assert_eq!(res[0].positions[0].avg_max_LTV.to_string(), String::from("0.65"));
        }

        // #[test]
        fn two_collateral_cdp_LTV_tests_bignums() {
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
                            denom: "double_debit".to_string(),
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
                            denom: "double_debit".to_string(),
                        },
                        current_supply: Uint128::zero(),
                        debt_total: Uint128::zero(),
                        supply_cap_ratio: Decimal::percent(100),
                        lp: false,
                        stability_pool_ratio_for_debt_cap: None,
                    },
                ]),
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let admin_addr = get_admin_addr(&app);
            app.execute(admin_addr, cosmos_msg).unwrap();

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
                    vec![
                        Coin {
                            denom: "debit".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                        },
                        Coin {
                            denom: "double_debit".to_string(),
                            amount: Uint128::from(10_000_000_000_000u128),
                        },
                    ],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Increase Debt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(249_995_000000u128)),
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

            //Query for Insolvency
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
            //Assert LTVs
            assert_eq!(res[0].positions[0].avg_borrow_LTV.to_string(), String::from("0.55"));
            assert_eq!(res[0].positions[0].avg_max_LTV.to_string(), String::from("0.65"));
        }
    } // mod cdp
} // mod tests
