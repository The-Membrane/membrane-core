pub mod tests {
    //! Accrual Tests
    //!
    //! This test suite covers debt accrual, credit repayment price accrual,
    //! discounted debt accrual, and the interaction between redemption rates
    //! and interest rates.

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};
    use membrane::cdp::{
        BasketPositionsResponse, CollateralInterestResponse, Config, EditBasket, ExecuteMsg,
        InterestResponse, QueryMsg, UpdateConfig,
    };
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, Basket, LPAssetInfo, PoolInfo, PoolType,
        SupplyCap, TWAPPoolInfo, UserInfo,
    };
    use cosmwasm_std::{
        attr, coin, to_json_binary, Addr, Binary, BlockInfo, Coin, Decimal, Empty, Response,
        StdError, StdResult, Uint128,
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
        fn accrue_discounted_debt() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Re-set Discount contract from config (it is already set during proper_instantiate)
            let config: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap();
            let msg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                chain_proxy: None,
                debt_auction: None,
                staking_contract: None,
                ltv_disco: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: config.discounts_contract.as_ref().map(|a| a.to_string()),
                liq_fee: None,
                affiliate_fee_max: None,
                debt_minimum: None,
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
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

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
                    max_borrow_LTV: Decimal::percent(50),
                    max_LTV: Decimal::percent(70),
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
                        lp: true,
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

            //Initial Deposit
            //Current Position: 100_000_000_000_000_000_000 lp_denom
            let msg = ExecuteMsg::Deposit {
                position_owner: Some("discounty".to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "lp_denom".to_string(),
                        amount: Uint128::from(100_000_000_000_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            //Current Position: 100_000_000_000_000_000_000_000 lp_denom -> 99_999_000000 credit_fulldenom
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(99_999_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("discounty"), cosmos_msg)
                .unwrap();

            //Accrue position bc queries no longer accrue new rates
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: Some("discounty".to_string()),
            };
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("discounty"), cosmos_msg)
                .unwrap();

            //Check interest rates
            //Last Rate: 14.2%
            let query_msg = QueryMsg::GetCollateralInterest {};
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                format!("{:?}", res.rates),
                String::from(
                    "[Decimal(0.142857142857142857), Decimal(0.166666666666666666), Decimal(0.125), Decimal(0.142857142857142857)]"
                )
            );

            //Assert the 90% discount on rates
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::new(1),
                    position_owner: "discounty".to_string(),
                }),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].positions[0].credit_amount,
                Uint128::new(11428_4571428)
            );
        }

        #[test]
        fn accrue_credit_repayment_price() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, true, true);

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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: Some(Decimal::percent(1)),
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
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999_000000u128)),
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
                &[coin(49_999_000000, "credit_fulldenom")],
            )
            .unwrap();

            //Insolvent position error
            ///Expected to Error due to a greater repayment price
            /// //otherwise this would be solvent and a valid increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(1_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("test"), cosmos_msg)
                .unwrap_err();

            //Successful repayment up to the new minimum debt
            //With only repayment price increases, the amount being repaid doesn't change..
            //..but the amount that results in minimum debt errors decreases
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(49_000_000000, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            //Assert Increased credit price is saved correctly
            let query_msg = QueryMsg::GetBasket {};
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.credit_price.price.to_string(),
                String::from("1")
            );

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::new(1),
                    position_owner: app.api().addr_make("test").to_string(),
                }),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].positions[0].credit_amount,
                Uint128::new(2427_542857)
            );

            //Insolvent withdrawal at that brings position to previous debt minimum
            ////This wouldn't be insolvent if there wasn't an increased repayment price
            /// 1960 backed by 3920: 50% borrow LTV so would've been solvent at $1 credit
            let msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::from(96_080_000000u128),
                }],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64),
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("test"), cosmos_msg)
                .unwrap_err();

            //////////////NEGATIVE RATES///////
            ///
            /// ///////
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

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
                base_interest_rate: None,
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: Some(Decimal::percent(1)),
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
                        amount: Uint128::from(100_000_000000u128),
                    }],
                )
                .unwrap();
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(49_999_000000u128)),
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
                &[coin(49_999_000000, "credit_fulldenom")],
            )
            .unwrap();

            ///Expected to pass due to a lower repayment price
            /// //otherwise this would be insolvent
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();

            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(app.api().addr_make("test"), cosmos_msg).unwrap();

            //Successful repayment up to the new minimum debt
            //With repayment price decreases, the amount being repaid doesn't change..
            //..but the amount that results in minimum debt errors increases from 2000 to 2002
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(47_999_000000, "credit_fulldenom")])
                .unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let err = app
                .execute(app.api().addr_make("test"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Position's debt (600) is below minimum: 2000")
            );

            //Assert decreased credit price is saved correctly
            //After 3 years
            let query_msg = QueryMsg::GetBasket {};
            let res: Basket = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.credit_price.price.to_string(),
                String::from("0.7")
            );

            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::new(1),
                    position_owner: app.api().addr_make("test").to_string(),
                }),
            };
            let res: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res[0].positions[0].credit_amount,
                Uint128::new(50001_000000)
            );

            //Query Redemption Rate/Repayment Interest
            let resp: InterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::GetCreditRate {})
                .unwrap();

            assert_eq!(
                resp.credit_interest.to_string(),
                String::from("0.571428571428571428"),
            );
            assert_eq!(resp.negative_rate, true,);
        }

        #[test]
        fn accrue_repayment_rate_to_interest_rate() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, true);

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
                cpc_margin_of_error: Some(Decimal::percent(1)),
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(get_admin_addr(&app), cosmos_msg).unwrap();

            //Initial Deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: None,
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
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful Increase
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(50_000_000_000u128)),
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

            //Accrue position bc queries no longer accrue new rates
            let msg = ExecuteMsg::Accrue {
                position_ids: vec![Uint128::new(1u128)],
                position_owner: None,
            };
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Assert interest rate is the LTV-based rate (raw, not net of credit price)
            //Rate = base_rate / max_LTV = 0.1 / 0.7 = 0.142857
            //Note: Credit price appreciation is applied separately during accrual, not stored in collateral rates
            let query_msg = QueryMsg::GetCollateralInterest {};
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            let res: CollateralInterestResponse = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(
                res.rates[0].to_string(),
                String::from("0.142857142857142857")
            );
        }
    }
}
