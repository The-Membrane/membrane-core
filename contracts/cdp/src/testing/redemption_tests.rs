pub mod tests {

    mod cdp {
        use super::*;

        use std::str::FromStr;

        use crate::helpers::{CDPContract, LQContract};
        use membrane::cdp::{
            ExecuteMsg, QueryMsg, EditBasket, UpdateConfig, Config, BasketPositionsResponse,
            RedeemabilityResponse,
        };
        use membrane::types::{
            cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo, PoolType,
            SupplyCap, Basket, MultiAssetSupplyCap, LPAssetInfo, PoolInfo,
        };
        use cosmwasm_std::{
            attr, coin, to_json_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError,
            StdResult, Uint128, BlockInfo,
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
        fn edit_redemption_info() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

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


                // revenue_destinations: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000_000000, "debit")])
                .unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
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
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful deposit: #2
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000_000000, "2nddebit")])
                .unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt: #2
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(50_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            ////Set Redemption Info///
            //Error: Premium too high
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![],
                redeemable: Some(true),
                premium: Some(100),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Custom Error val: \"Premium can't be greater than 99\"")
            );

            //Error: Loan repayment too high
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![],
                redeemable: Some(true),
                premium: Some(99),
                max_loan_repayment: Some(Decimal::percent(101)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from(
                    "Custom Error val: \"Max loan repayment can't be 0% or greater than 100%\""
                )
            );

            //Success, set redeemable
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: None,
                restricted_collateral_assets: Some(vec![String::from("debit")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Query Basket Redeemability
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .remaining_loan_repayment,
                Uint128::new(50_000_000000)
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0].position_id,
                Uint128::one()
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .restricted_collateral_assets,
                vec![String::from("debit")]
            );
            assert_eq!(res.premium_infos[0].premium, 10u128);
            assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);
            assert_eq!(
                res.premium_infos[0].users_of_premium[0]
                    .position_infos
                    .len(),
                1
            );

            //Success, turn restrictions off
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: None,
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: Some(vec![]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Query Basket Redeemability
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: Some(app.api().addr_make(USER).to_string()),
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .restricted_collateral_assets
                    .len(),
                0
            );

            //////Edit Redeemability/////
            //Error: Not the owner of said position or it doesnt exist in the User's redemption list
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::zero()],
                redeemable: None,
                premium: Some(2),
                max_loan_repayment: Some(Decimal::one()),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Position doesn't exist: 0")
            );

            //No ID specified
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![],
                redeemable: None,
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Custom Error val: \"Position IDs must be specified\"")
            );

            //Repetitive IDs
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one(), Uint128::one()],
                redeemable: None,
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Custom Error val: \"Position IDs must be unique\"")
            );

            //Toggle Redeemability (to and fro)
            //Off
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(false),
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            //None are added to the Response bc there are no premiums that aren't empty
            assert_eq!(res.premium_infos.len(), 0);

            //Error bc no premium set when setting to TRUE
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

            //On
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            assert_eq!(res.premium_infos[0].users_of_premium.len(), 1);

            //Only Edit Premium 1 and Set premium of Position 2
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one(), Uint128::new(2)],
                redeemable: None,
                premium: Some(20),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();

            assert_eq!(res.premium_infos.len(), 1);
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0].position_id,
                Uint128::one()
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[1].position_id,
                Uint128::new(2)
            );
            assert_eq!(res.premium_infos[0].premium, 20u128);

            ////Add a position to an existing user premium list///
            //turn redeemability Off for 1
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(false),
                premium: None,
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: None,
                premium: Some(20),
                max_loan_repayment: None,
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            ////Edit Max Loan Repayment && restricted_assets///
            //Error: Invalid restriciton
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one(), Uint128::new(2)],
                redeemable: None,
                premium: None,
                max_loan_repayment: Some(Decimal::percent(50)),
                restricted_collateral_assets: Some(vec![String::from("I_don't_want_this_asset")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            let err = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Custom Error val: \"Invalid restricted asset, only the position's collateral assets are viable to restrict\"")
            );

            //Success
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one(), Uint128::new(2)],
                redeemable: None,
                premium: None,
                max_loan_repayment: Some(Decimal::percent(50)),
                restricted_collateral_assets: Some(vec![String::from("debit")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Assert Redeemability change
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .remaining_loan_repayment,
                Uint128::new(25000_000000)
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[1]
                    .remaining_loan_repayment,
                Uint128::new(25000_000000)
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .restricted_collateral_assets,
                vec![String::from("debit")]
            );
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[1]
                    .restricted_collateral_assets,
                vec![String::from("debit")]
            );
        }

        #[test]
        fn redemption_w_multiple_premiums() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let res: Config = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
                .unwrap();
            let op_contract = res.chain_proxy.unwrap();

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


                // revenue_destinations: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000_000000, "debit")])
                .unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
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
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful deposit: #2
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000_000000, "debit")])
                .unwrap();
            let res = app
                .execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Successful IncreaseDebt: #2
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
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

            ////Redeem///// (No enabled positions yet)
            //Error
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(100_000_000000, "credit_fulldenom")],
                )
                .unwrap();
            let err = app
                .execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from(
                    "Custom Error val: \"No collateral to redeem with at this max premium\""
                )
            );

            //Set #1 to 10% premium
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Set #2 to 20% premium
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::new(2)],
                redeemable: Some(true),
                premium: Some(20),
                max_loan_repayment: Some(Decimal::percent(20)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            ////Redeem///// (15k max currently)
            //Error: Wrong asset
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: None,
            };
            let cosmos_msg = cdp_contract
                .call(redemption_msg.clone(), vec![coin(1, "not_redeemable")])
                .unwrap();
            let err = app
                .execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Custom Error val: \"Must send only the Basket's debt token\"")
            );

            //Repay 46k of the 50k debt in position 1 to lower the available redemption amount to 4k or 14k total
            let msg = ExecuteMsg::Repay {
                position_id: Uint128::one(),
                position_owner: Some(app.api().addr_make(USER).to_string()),
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract
                .call(msg, vec![coin(46_000_000000, "credit_fulldenom")])
                .unwrap();
            app.execute(app.api().addr_make("bigger_bank"), cosmos_msg)
                .unwrap();

            //Success, but Send too much (15k max currently)
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(100_000_000000, "credit_fulldenom")],
                )
                .unwrap();
            app.execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap();

            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap()
                    .query_all_balances(app.api().addr_make("redeemer"))
                    .unwrap(),
                vec![
                    coin(87940_000000, "credit_fulldenom"),
                    coin(9800_000000, "debit"),
                    coin(1, "not_redeemable")
                ]
            );

            //Assert Positions were updated
            let position: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(
                    &cdp_contract.addr(),
                    &QueryMsg::GetBasketPositions {
                        start_after: None,
                        limit: None,
                        user: None,
                        user_info: Some(UserInfo {
                            position_id: Uint128::new(1),
                            position_owner: app.api().addr_make(USER).to_string(),
                        }),
                    },
                )
                .unwrap();
            assert_eq!(
                position[0].positions[0].collateral_assets[0].asset.amount,
                Uint128::new(98200_000000)
            );
            assert_eq!(
                position[0].positions[0].credit_amount,
                Uint128::new(2000_000000)
            );

            let position_2: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(
                    &cdp_contract.addr(),
                    &QueryMsg::GetBasketPositions {
                        start_after: None,
                        limit: None,
                        user: None,
                        user_info: Some(UserInfo {
                            position_id: Uint128::new(2),
                            position_owner: app.api().addr_make("bigger_bank").to_string(),
                        }),
                    },
                )
                .unwrap();
            assert_eq!(
                position_2[0].positions[0].collateral_assets[0].asset.amount,
                Uint128::new(92_000_000000)
            );
            assert_eq!(
                position_2[0].positions[0].credit_amount,
                Uint128::new(40000_000000)
            );
        }

        //Redemption test with multiple collateral in the position (debit & LP bc they r priced differently)
        //Tests max premium as well
        #[test]
        fn redemption_w_multiple_collateral() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            //Add 2nddebit as collateral
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: Some(cAsset {
                    asset: Asset {
                        info: AssetInfo::NativeToken {
                            denom: "2nddebit".to_string(),
                        },
                        amount: Uint128::zero(),
                    },
                    max_borrow_LTV: Decimal::percent(50),
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


                // revenue_destinations: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

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


                // revenue_destinations: None,
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


                // revenue_destinations: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Add supply caps and a new cAsset (lp_denom)
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
                base_interest_rate: Some(Decimal::percent(5)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,


                // revenue_destinations: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

            //Send lp_denom
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                app.api().addr_make(USER),
                &[coin(100_000_000_000_000_000_000_000, "lp_denom")],
            )
            .unwrap();

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    exec_msg.clone(),
                    vec![
                        coin(100_000000_000, "debit"),
                        coin(100_000_000_000_000_000_000_000, "lp_denom"),
                    ],
                )
                .unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
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
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            // Skip 2222222 seconds
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(2222222),
                chain_id: app.block_info().chain_id,
            });

            //Set #1 to 10% premium
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: None,
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            ////DEPOSIT A 2ND POSITION THAT HAS FULLY RESTRICTED ASSETS, SHOULDNT CHANGE THE OUTCOMES

            //Successful deposit
            let exec_msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(exec_msg.clone(), vec![coin(100_000_000000, "2nddebit")])
                .unwrap();
            let res = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Successful IncreaseDebt
            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(50_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Set same redeemability + restrict all collateral assets
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::new(2)],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(10)),
                restricted_collateral_assets: Some(vec![String::from("2nddebit")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            ////Redeem from a multi-collateral position///
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: Some(9),
            };
            //Error: Nothing redeemed since premium is too low
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(100_000_000000, "credit_fulldenom")],
                )
                .unwrap();
            let err = app
                .execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from(
                    "Custom Error val: \"No collateral to redeem with at this max premium\""
                )
            );

            //Assert pre-revenue contract5 balances
            assert_eq!(
                app.wrap().query_all_balances(&{ let cfg: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap(); cfg.staking_contract.unwrap().to_string() }).unwrap(),
                vec![]
            );

            //Successful redemption
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: Some(10),
            };
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(100_000_000000, "credit_fulldenom")],
                )
                .unwrap();
            app.execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap();

            //Assert revenue was sent to contract5
            assert_eq!(
                app.wrap().query_all_balances(&{ let cfg: Config = app.wrap().query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {}).unwrap(); cfg.staking_contract.unwrap().to_string() }).unwrap(),
                vec![coin(276664990, "credit_fulldenom")] //276664990 (total rev) - 25_000_000 (0.5% redemption fee rev) = _ rate rev
            );

            //Assert pending revenue is back to 0
            let query_msg = QueryMsg::GetBasket {};
            let res = app
                .wrap()
                .query_wasm_smart::<Basket>(cdp_contract.addr(), &query_msg.clone())
                .unwrap();
            assert_eq!(res.pending_revenue.total_pending, Uint128::zero());

            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap()
                    .query_all_balances(app.api().addr_make("redeemer"))
                    .unwrap(),
                vec![
                    coin(94975_000000, "credit_fulldenom"), //excess (redeemed 5000_000000 + paid 25_000_000 fee)
                    //user pays 5k for 4500 worth
                    coin(1499_999999, "debit"),                    // (1499)
                    coin(1499_999999999999998500, "lp_denom"),     //(1499 * 2)
                    coin(1, "not_redeemable")
                ]
            );

            //Assert Positions were updated
            let position: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(
                    &cdp_contract.addr(),
                    &QueryMsg::GetBasketPositions {
                        start_after: None,
                        limit: None,
                        user: None,
                        user_info: Some(UserInfo {
                            position_id: Uint128::new(1),
                            position_owner: app.api().addr_make(USER).to_string(),
                        }),
                    },
                )
                .unwrap();
            assert_eq!(
                position[0].positions[0].collateral_assets[0].asset.amount,
                Uint128::new(98500_000001)
            );
            assert_eq!(
                position[0].positions[0].collateral_assets[1].asset.amount,
                Uint128::new(98500_000000000000001500)
            );
            assert_eq!(
                position[0].positions[0].credit_amount,
                Uint128::new(45251_664990)
            ); //accrued debt

            //Assert remaining loan repayment is 0'd
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            //Bc the remaining loan repayment is 0'd, the 1st position is not redeemable
            //The 2nd position got deleted bc it has fully restricted assets
            assert_eq!(
                res.premium_infos[0].users_of_premium[0]
                    .position_infos
                    .len(),
                0 as usize
            );

            //Reset #1's loan repayment cap but restrict the collateral to only debit
            let redemption_msg = ExecuteMsg::EditRedeemability {
                position_ids: vec![Uint128::one()],
                redeemable: Some(true),
                premium: Some(10),
                max_loan_repayment: Some(Decimal::percent(100)),
                restricted_collateral_assets: Some(vec![String::from("lp_denom")]),
            };
            let cosmos_msg = cdp_contract.call(redemption_msg.clone(), vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

            //Assert the reset loan repayment cap
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .remaining_loan_repayment,
                Uint128::new(45251_664990)
            );

            //Successful restricted redemption
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(50000_000000, "credit_fulldenom")],
                )
                .unwrap();
            app.execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap();

            //Assert that the collateral & excess was sent to the redeemer
            assert_eq!(
                app.wrap()
                    .query_all_balances(app.api().addr_make("redeemer"))
                    .unwrap(),
                vec![
                    coin(51507_076685, "credit_fulldenom"), // redeemed 43,050,333,994 - 215,251,669.97 fee
                    coin(40426_498490, "debit"),            //40426 - 1499 = 38927 new debit redeemed
                    coin(1499_999999999999998500, "lp_denom"), //No extra since restricted
                    coin(1, "not_redeemable")
                ]
            );

            //Assert Positions were updated & debt minimum was held
            let position: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(
                    &cdp_contract.addr(),
                    &QueryMsg::GetBasketPositions {
                        start_after: None,
                        limit: None,
                        user: None,
                        user_info: Some(UserInfo {
                            position_id: Uint128::new(1),
                            position_owner: app.api().addr_make(USER).to_string(),
                        }),
                    },
                )
                .unwrap();
            assert_eq!(
                position[0].positions[0].collateral_assets[0].asset.amount,
                Uint128::new(59573_501510)
            );
            assert_eq!(
                position[0].positions[0].collateral_assets[1].asset.amount,
                Uint128::new(98500_000000000000001500)
            );
            assert_eq!(
                position[0].positions[0].credit_amount,
                Uint128::new(2000_000000)
            );

            //Assert Pos #1 remaining loan repayment is updated from a partial full
            let query_msg = QueryMsg::GetBasketRedeemability {
                position_owner: None,
                start_after: None,
                limit: None,
            };
            let res = app
                .wrap()
                .query_wasm_smart::<RedeemabilityResponse>(
                    cdp_contract.addr(),
                    &query_msg.clone(),
                )
                .unwrap();
            //this isn't 2k bc the accrual happens during the redemption, not before the edit redemption
            assert_eq!(
                res.premium_infos[0].users_of_premium[0].position_infos[0]
                    .remaining_loan_repayment,
                Uint128::new(2000_000000)
            );

            //Attempt to redeem the rest of the debt even tho its at the minimum: error
            let redemption_msg = ExecuteMsg::RedeemCollateral {
                max_collateral_premium: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    redemption_msg.clone(),
                    vec![coin(45000_000000, "credit_fulldenom")],
                )
                .unwrap();
            app.execute(app.api().addr_make("redeemer"), cosmos_msg)
                .unwrap_err();
        }
    }
}
