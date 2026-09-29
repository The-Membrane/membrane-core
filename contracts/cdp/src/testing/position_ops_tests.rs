pub mod tests {

    mod cdp {
        use super::*;

        use std::str::FromStr;

        use crate::helpers::{CDPContract, LQContract};
        use membrane::cdp::{
            ExecuteMsg, QueryMsg, EditBasket, UpdateConfig, Config,
            BasketPositionsResponse, CollateralInterestResponse, PositionResponse,
        };
        use membrane::types::{
            cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo,
            PoolType, SupplyCap, Basket, MultiAssetSupplyCap, LPAssetInfo, PoolInfo,
        };
        use cosmwasm_std::{
            attr, coin, coins, to_json_binary, Addr, Binary, Coin, Decimal, Empty,
            Response, StdError, StdResult, Uint128, BlockInfo,
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

        #[test]
        fn misc_query() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);

            //Edit Basket 1
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Initial deposit to Basket 1
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("sender88").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            app.send_tokens(
                app.api().addr_make("little_bank"),
                app.api().addr_make("sender88"),
                &vec![coin(22, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(11, "debit")])
                .unwrap();
            let res = app
                .execute(app.api().addr_make("sender88"), cosmos_msg)
                .unwrap();

            //Simulate LTV mint Query
            let msg = QueryMsg::SimulateMint {
                position_info: UserInfo {
                    position_id: Uint128::one(),
                    position_owner: app.api().addr_make("sender88").to_string(),
                },
                LTV: Decimal::percent(50),
            };
            let resp: Uint128 = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp,
                Uint128::new(5),
            );

            //Query BasketPositions
            let msg = QueryMsg::GetBasketPositions {
                start_after: Some(app.api().addr_make("sender88").to_string()),
                limit: None,
                user: None,
                user_info: None,
            };

            let resp: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp.len().to_string(),
                String::from("0"),
            );

            //Query UserPositions
            let msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: Some(app.api().addr_make("sender88").to_string()),
                user_info: None,
            };
            let resp: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &msg.clone())
                .unwrap();
            assert_eq!(
                resp[0].positions[0],
                PositionResponse {
                    position_id: Uint128::new(1),
                    collateral_assets: vec![
                        cAsset {
                            asset: Asset {
                                info: AssetInfo::NativeToken {
                                    denom: "debit".to_string(),
                                },
                                amount: Uint128::from(11u128),
                            },
                            max_borrow_LTV: Decimal::percent(50),
                            max_LTV: Decimal::percent(70),
                            pool_info: None,
                            rate_index: Decimal::one(),
                            peg_rate_index: Decimal::one(),
                                    force_redemptions: None,
                                    max_threshold_to_delay: Decimal::percent(15),
                        }
                    ],
                    cAsset_ratios: vec![], //arent calc'd in queries anymore
                    credit_amount: Uint128::zero(),
                    rate_segments: vec![],
                    avg_borrow_LTV: Decimal::percent(0), //arent calc'd in queries anymore
                    avg_max_LTV: Decimal::percent(0), //arent calc'd in queries anymore
                    deployed_to: vec![],
                    pending_interest: Uint128::zero(),
                    total_interest_accrued: Uint128::zero(),
                    peg_rate_segments: vec![],
                },
            );

            //Update Config
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: Some(app.api().addr_make("new_owner").to_string()),
                chain_proxy: Some(app.api().addr_make("new_op").to_string()),
                debt_auction: Some(app.api().addr_make("new_auction").to_string()),
                staking_contract: Some(app.api().addr_make("new_staking").to_string()),
                oracle_contract: Some(app.api().addr_make("new_oracle").to_string()),
                liquidity_contract: Some(app.api().addr_make("new_liq_check").to_string()),
                discounts_contract: Some(app.api().addr_make("new_dc").to_string()),
                liq_fee: Some(Decimal::percent(13)),
                ltv_disco: Some(app.api().addr_make("new_ltv_disco").to_string()),
                affiliate_fee_max: None,
                debt_minimum: Some(Uint128::zero()),
                base_debt_cap_multiplier: Some(Uint128::new(48497)),
                oracle_time_limit: Some(33u64),
                credit_twap_timeframe: Some(33u64),
                collateral_twap_timeframe: Some(33u64),
                cpc_multiplier: Some(Decimal::percent(50)),
                rate_slope_multiplier: Some(Decimal::percent(2)),
                skip_credit_price_accrual: Some(false),
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            let resp: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}.clone())
                .unwrap();

            assert_eq!(resp.owner, app.api().addr_make("new_owner"));
            assert_eq!(resp.staking_contract, Some(app.api().addr_make("new_staking")));
            assert_eq!(resp.oracle_contract, Some(app.api().addr_make("new_oracle")));
            assert_eq!(resp.liquidity_contract, Some(app.api().addr_make("new_liq_check")));
            assert_eq!(resp.discounts_contract, Some(app.api().addr_make("new_dc")));
            assert_eq!(resp.ltv_disco, app.api().addr_make("new_ltv_disco"));
            assert_eq!(resp.chain_proxy, Some(app.api().addr_make("new_op")));
            assert_eq!(resp.debt_auction, Some(app.api().addr_make("new_auction")));
            assert_eq!(resp.liq_fee, Decimal::percent(13));
            assert_eq!(resp.debt_minimum, Uint128::zero());
            assert_eq!(resp.base_debt_cap_multiplier, Uint128::new(48497));
            assert_eq!(resp.oracle_time_limit, 33u64);
            assert_eq!(resp.credit_twap_timeframe, 33u64);
            assert_eq!(resp.collateral_twap_timeframe, 33u64);
            assert_eq!(resp.cpc_multiplier, Decimal::percent(50));
            assert_eq!(resp.rate_slope_multiplier, Decimal::percent(2));
            assert_eq!(resp.affiliate_fee_max, Decimal::percent(10));
            assert_eq!(resp.revenue_distributor, None);
            assert_eq!(resp.skip_credit_price_accrual, false);

            //Update owner after new owner calls the function
            //Update Config
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                chain_proxy: None,
                debt_auction: None,
                staking_contract: None,
                ltv_disco: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                affiliate_fee_max: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                oracle_time_limit: None,
                credit_twap_timeframe: None,
                collateral_twap_timeframe: None,
                cpc_multiplier: None,
                rate_slope_multiplier: Some(Decimal::percent(3)),
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("new_owner"), cosmos_msg).unwrap();

            let resp: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}.clone())
                .unwrap();

            assert_eq!(resp.owner, app.api().addr_make("new_owner"));
            assert_eq!(resp.chain_proxy, Some(app.api().addr_make("new_op")));
            assert_eq!(resp.debt_auction, Some(app.api().addr_make("new_auction")));
            assert_eq!(resp.staking_contract, Some(app.api().addr_make("new_staking")));
            assert_eq!(resp.oracle_contract, Some(app.api().addr_make("new_oracle")));
            assert_eq!(resp.liquidity_contract, Some(app.api().addr_make("new_liq_check")));
            assert_eq!(resp.discounts_contract, Some(app.api().addr_make("new_dc")));
            assert_eq!(resp.liq_fee, Decimal::percent(13));
            assert_eq!(resp.debt_minimum, Uint128::zero());
            assert_eq!(resp.base_debt_cap_multiplier, Uint128::new(48497));
            assert_eq!(resp.oracle_time_limit, 33u64);
            assert_eq!(resp.credit_twap_timeframe, 33u64);
            assert_eq!(resp.collateral_twap_timeframe, 33u64);
            assert_eq!(resp.cpc_multiplier, Decimal::percent(50));
            assert_eq!(resp.rate_slope_multiplier, Decimal::percent(3));
            assert_eq!(resp.affiliate_fee_max, Decimal::percent(10));
            assert_eq!(resp.revenue_distributor, None);
            assert_eq!(resp.skip_credit_price_accrual, false);
        }

        #[test]
        fn edit_cAsset() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);

            //Invalid Asset
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "not_debit".to_string(),
                },
                max_borrow_LTV: None,
                max_LTV: None,
                max_threshold_to_delay: None,
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap_err();

            //Add liq-queue to the initial basket
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
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
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Successfull edit
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: Some(Decimal::percent(82)),
                max_LTV: Some(Decimal::percent(83)),
                max_threshold_to_delay: None,
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let res = app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Query Basket
            let resp: Basket = app
                .wrap()
                .query_wasm_smart(
                    cdp_contract.addr(),
                    &QueryMsg::GetBasket {},
                )
                .unwrap();

            assert_eq!(
                resp.collateral_types[0].max_borrow_LTV,
                Decimal::percent(82)
            );
            assert_eq!(resp.collateral_types[0].max_LTV, Decimal::percent(83));

            //Error: Borrow LTV too high
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: Some(Decimal::percent(100)),
                max_LTV: Some(Decimal::percent(100)),
                max_threshold_to_delay: None,
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let err = app.execute(admin_addr.clone(), cosmos_msg).unwrap_err();

            //Successfull edit
            let edit_msg = ExecuteMsg::EditcAsset {
                asset: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                max_borrow_LTV: None,
                max_LTV: Some(Decimal::percent(100)),
                max_threshold_to_delay: None,
            };
            let cosmos_msg = cdp_contract.call(edit_msg, vec![]).unwrap();
            let res = app.execute(admin_addr.clone(), cosmos_msg).unwrap();
        }

        #[test]
        fn open_position_deposit() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);
            let user_addr = get_user_addr(&app);

            //Add supply caps and a new cAsset (2nddebit)
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                            denom: "2nddebit".to_string(),
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Testing Position creation

            //Invalid id test
            let error_exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: Some(Uint128::from(3u128)),
                affiliate_address: None,
                affiliate_label: None,
            };

            //Fail due to a non-existent position
            //First msg deposits since no positions were initially found, meaning the _id never got tested
            app.send_tokens(
                app.api().addr_make("little_bank"),
                app.api().addr_make("owner"),
                &vec![coin(22, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(error_exec_msg, vec![coin(11, "debit")])
                .unwrap();
            app.execute(app.api().addr_make("owner"), cosmos_msg.clone())
                .unwrap();
            app.execute(app.api().addr_make("owner"), cosmos_msg)
                .unwrap_err();

            //Fail due to invalid collateral
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(666, "fake_debit")])
                .unwrap();
            app.execute(app.api().addr_make("faker"), cosmos_msg)
                .unwrap_err();

            //Successful attempt
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            app.send_tokens(
                app.api().addr_make(USER),
                app.api().addr_make("owner"),
                &vec![coin(11, "2nddebit"), coin(11, "debit")],
            )
            .unwrap();
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(11, "debit"), coin(11, "2nddebit")])
                .unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg).unwrap();

            let response = res
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "method" && attr.value == "deposit"))
                .ok_or_else(|| panic!("unable to find deposit event"))
                .unwrap();

            assert_eq!(
                response.attributes[1..4],
                vec![
                    attr("method", "deposit"),
                    attr("position_owner", &app.api().addr_make("owner").to_string()),
                    attr("position_id", "2"),
                ]
            );
            assert_eq!(response.attributes[4].value, String::from("[Asset { info: NativeToken { denom: \"debit\" }, amount: Uint128(11) }, Asset { info: NativeToken { denom: \"2nddebit\" }, amount: Uint128(11) }]") );

            //Test max Position amount
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(1, "debit")])
                .unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap();
            //This one should fail
            let res = app.execute(app.api().addr_make("owner"), cosmos_msg.clone()).unwrap_err();
        }

        #[test]
        fn repay() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);
            let user_addr = get_user_addr(&app);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //NoUserPositions Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract.call(repay_msg, vec![]).unwrap();
            let res = app
                .execute(app.api().addr_make("sender88"), cosmos_msg)
                .unwrap_err();

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Invalid Collateral Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: Some(app.api().addr_make(USER).to_string()),
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(666, "fake_debit")])
                .unwrap();
            let res = app
                .execute(app.api().addr_make("faker"), cosmos_msg)
                .unwrap_err();

            //NonExistent Position Error
            let repay_msg = ExecuteMsg::Repay {
                position_id: Uint128::from(3u128),
                position_owner: Some(app.api().addr_make(USER).to_string()),
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(repay_msg, vec![coin(111, "credit_fulldenom")])
                .unwrap();
            let res = app
                .execute(app.api().addr_make("coin_God"), cosmos_msg)
                .unwrap_err();
        }

        #[test]
        fn increase_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //NoUserPositions Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(admin_addr.clone(), cosmos_msg).unwrap_err();

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg, vec![coin(11_000_000_000_000, "debit")])
                .unwrap();
            let res = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //NonExistentPosition Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(3u128),
                amount: Some(Uint128::from(1u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: Insolvent Error
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: Some(Decimal::percent(100)),
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: No amount inputs
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap_err();

            //Increase_debt by LTV: Success
            let increase_debt_msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: None,
                LTV: Some(Decimal::percent(40)),
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap();

            //Query indebted position
            //    let query_msg = QueryMsg::GetPositionInsolvency {
            //     position_id: Uint128::new(1),
            //     position_owner: String::from("bigger_bank"),
            //     };
            //     let res: InsolvencyResponse = app
            //         .wrap()
            //         .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //         .unwrap();
            //     //Assert LTV
            //     assert_eq!(
            //         res.insolvent_positions,
            //         vec![InsolventPosition {
            //             insolvent: false,
            //             position_info: UserInfo {
            //                 position_id: Uint128::new(1),
            //                 position_owner: String::from("bigger_bank"),
            //             },
            //             current_LTV: Decimal::percent(40),
            //             available_fee: Uint128::zero(),
            //         }]
            //     );
        }

        #[test]
        fn withdrawal_errors() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);
            let user_addr = get_user_addr(&app);

            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            let valid_assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::from(5u128),
            }];

            //User has no positions in the basket error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: valid_assets.clone(),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(admin_addr.clone(), cosmos_msg).unwrap_err();

            //Initial Deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, coins(11, "debit")).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Non-existent position error but user still has positions in the basket
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(3u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::zero(),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //Invalid collateral fail
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "notdebit".to_string(),
                },
                amount: Uint128::from(10u128),
            }];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: assets.clone(),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //Withdrawing too much error
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "debit".to_string(),
                },
                amount: Uint128::from(333333333u128),
            }];

            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets,
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
        }

        #[test]
        fn asset_expunge() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);
            let user_addr = get_user_addr(&app);

            //Add supply caps & a 2nd cAsset
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(0u128),
                    },
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
                    pool_info: None,
                    rate_index: Decimal::one(),
                    peg_rate_index: Decimal::one(),
                    force_redemptions: None,
                    max_threshold_to_delay: Decimal::percent(15),
                }),
                liq_queue: None,
                credit_pool_infos: None,
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
                            denom: "2nddebit".to_string(),
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Initial deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract.call(exec_msg, vec![coin(11, "debit"), coin(11, "2nddebit")]).unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Set debit supply caps to 0
            let edit_basket_msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(0),
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
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            //Attempt to withdraw 2nddebit only: Error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //Attempt to withdraw both but not debit fully: Error
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(1u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //Withdraw only debit partially: Successful
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(5u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Withdraw both: Successful
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::from(11u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(6u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Query Position assets to assert full withdrawal
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
            app
                .wrap()
                .query_wasm_smart::<BasketPositionsResponse>(cdp_contract.addr(), &query_msg.clone())
                .unwrap_err();
        }
    }
}
