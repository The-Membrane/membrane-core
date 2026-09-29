pub mod tests {
    //! Interest Rate and Revenue Test Suite
    //!
    //! This test suite covers:
    //! - Interest rate calculations for LP and standard collateral
    //! - Rate slope behavior based on supply cap ratios
    //! - Revenue accrual and distribution to staking
    //! - Forced redemption behavior for rate-hiked assets

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};
    use membrane::cdp::{
        EditBasket, ExecuteMsg, QueryMsg, Config,
        BasketPositionsResponse, CollateralInterestResponse,
    };
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo,
        PoolType, SupplyCap, Basket, MultiAssetSupplyCap, StakeDistribution,
        PoolInfo, LPAssetInfo,
    };
    use membrane::staking::Config as Staking_Config;
    use cosmwasm_std::{
        attr, coin, to_json_binary, Addr, Binary, Coin, Decimal, Empty,
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

    mod cdp {
        use super::*;

        #[test]
        fn interest_rates() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Add LP asset
            //Set supply caps
            //Set general parameters
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(99),
                    max_LTV: Decimal::percent(100),
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
                    force_redemptions: Some(true),
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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Initial Deposit for Position 1
            //Current Position: 100_000_000_000_000_000_000_000 lp_denom
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
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            // //Assert redemptions were set for a hiked rate asset
            // let query_msg = QueryMsg::GetBasketRedeemability {
            //     position_owner: None,
            //     start_after: None,
            //     limit: None,
            // };
            // let res = app
            //     .wrap()
            //     .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(0));
            // assert_eq!(res.premium_infos[0].premium, 1u128);
            // assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);
            // assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos.len(), 1);

            // //Error: Can't manually change if rate hiked asset
            // let redemption_msg = ExecuteMsg::EditRedeemability {
            //     position_ids: vec![Uint128::one()],
            //     redeemable: Some(true),
            //     premium: Some(99),
            //     max_loan_repayment: Some(Decimal::percent(100)),
            //     restricted_collateral_assets: None,
            // };
            // let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            // let err = app.execute(app.api().addr_make("bigger_bank"), cosmos_msg).unwrap_err();
            // assert_eq!(err.root_cause().to_string(), String::from("Custom Error val: \"Can't edit redemption for a position with a rate hike asset: NativeToken { denom: \\\"lp_denom\\\" }\""));

            //Successful Increase for Position 1
            //Current Position: 100_000_000_000_000_000_000_000 lp_denom -> 2_000_000000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2_000_000000u128)),
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

            // //Assert redemptions were set for a hiked rate asset
            // let query_msg = QueryMsg::GetBasketRedeemability {
            //     position_owner: None,
            //     start_after: None,
            //     limit: None,
            // };
            // let res = app
            //     .wrap()
            //     .query_wasm_smart::<RedeemabilityResponse>(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].remaining_loan_repayment, Uint128::new(2000000000));
            // assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos[0].position_id, Uint128::new(1));
            // assert_eq!(res.premium_infos[0].premium, 1u128);
            // assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);
            // assert_eq!(res.premium_infos[0].users_of_premium[0].position_infos.len(), 1);

            //Query Basket Debt Caps
            // let query_msg = QueryMsg::GetBasketDebtCaps { };
            // let res: Vec<DebtCap> = app
            //     .wrap()
            //     .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
            //     .unwrap();
            // assert_eq!(
            //     format!("{:?}", res),
            //     String::from("[DebtCap { collateral: NativeToken { denom: \"debit\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"base\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"quote\" }, debt_total: Uint128(0), cap: Uint128(0) }, DebtCap { collateral: NativeToken { denom: \"lp_denom\" }, debt_total: Uint128(2000000000), cap: Uint128(249995000001) }]")
            // );

            //Accrue position bc queries no longer accrue new rates
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Check lp_denom rate: LTV-based = base_rate * (1/max_LTV) = 0.1 * (1/1.0) = 0.1
            //Note: Supply cap influence on rates is disabled; rates are purely insurance-ratio/LTV-based
            let query_msg = QueryMsg::GetCollateralInterest {};
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[3].to_string(),
                String::from("0.1")
            );

            //Successful Increase for Position 1
            //Current Position: 100_000_000_000_000_000_000_000 lp_denom -> 198_000_000000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(196_000_000_000u128)),
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

            //Accrue position since query will use old rates if not
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Check lp_Denom rate unchanged (supply caps don't influence rates)
            let query_msg = QueryMsg::GetCollateralInterest {};
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[3].to_string(),
                String::from("0.1")
            );

            //Set LP denom supply cap (supply caps no longer influence rates)
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::from(198_000_000_000u128),
                        debt_total: Uint128::from(198_000_000_000u128),
                        supply_cap_ratio: Decimal::from_str("0.98").unwrap(),
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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Accrue position since query will use old rates if not
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Rate unchanged by supply cap changes
            let query_msg = QueryMsg::GetCollateralInterest {};
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[3].to_string(),
                String::from("0.1")
            );

            //Set LP denom supply cap extremely low (still no effect on rates)
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: Some(vec![
                    SupplyCap {
                        asset_info: AssetInfo::NativeToken {
                            denom: "lp_denom".to_string(),
                        },
                        current_supply: Uint128::from(198_000_000_000u128),
                        debt_total: Uint128::from(198_000_000_000u128),
                        supply_cap_ratio: Decimal::from_str("0.001").unwrap(),
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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Accrue position since query will use old rates if not
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Rate still unchanged: supply caps don't influence interest rates
            let query_msg = QueryMsg::GetCollateralInterest {};
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[3].to_string(),
                String::from("0.1")
            );
        }

        #[test]
        fn revenue() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let staking_contract = res.staking_contract.unwrap();

            //Edit Basket
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
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(app.api().addr_make("test").to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(100_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();
            //Send credit
            app.send_tokens(
                app.api().addr_make("sender"),
                app.api().addr_make("test"),
                &[coin(49_999_000_000, "credit_fulldenom")],
            )
            .unwrap();

            //Assert revenue is 0
            let query_msg = QueryMsg::GetBasket {};
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.pending_revenue.total_pending, Uint128::zero());

            //Skip a year
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });

            //Accrue position manually
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg)
                .unwrap();

            //Assert revenue
            let query_msg = QueryMsg::GetBasket {};
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.pending_revenue.total_pending, Uint128::new(7142714285u128));

            //Skip SP pre-revenue distribution check (stability pool removed)

            //Successful repayment that will leave the accrued interest left
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(49_000_000_000, "credit_fulldenom")])
                .unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            let query_msg = QueryMsg::GetBasket {};
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            //50% of the revenue stays in pending
            assert_eq!(res.pending_revenue.total_pending, Uint128::new(3571_357143u128));
            ////50% of the revenue is distributed///
            // Staking gets 10% of the revenue
            assert_eq!(
                app.wrap().query_all_balances(staking_contract).unwrap(),
                vec![coin(714_271428, "credit_fulldenom")]
            );
            //Skip SP revenue distribution assertion (stability pool removed)
            // 714271430 + 142854285 + 2857085714 = 3719215729  (total revenue)
        }
    }
}
