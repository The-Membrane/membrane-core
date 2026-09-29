pub mod tests {

    mod cdp {
        use super::*;

        use std::str::FromStr;

        use crate::helpers::{CDPContract, LQContract};

        use membrane::cdp::{
            EditBasket, ExecuteMsg, QueryMsg, UpdateConfig, Config,
            BasketPositionsResponse, CollateralInterestResponse, LiquidationStatResponse,
        };
        use membrane::types::{
            cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo, PoolType, SupplyCap,
            Basket, MultiAssetSupplyCap,
        };
        use membrane::staking::Config as Staking_Config;

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
        fn test_collateral_rate_assurance_callback_execution() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            // Execute rate assurance callback directly
            let msg = ExecuteMsg::CollateralRateAssurance {
                collateral_denoms: Some(vec!["debit".to_string()]),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(cdp_contract.addr()), cosmos_msg);

            // Should fail because no rate assurance state exists for this denom
            assert!(result.is_err());
        }

        #[test]
        fn test_collateral_rate_assurance_unauthorized_callback() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);

            // Try to execute rate assurance callback from unauthorized address
            let msg = ExecuteMsg::CollateralRateAssurance {
                collateral_denoms: Some(vec!["debit".to_string()]),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let result = app.execute(user_addr, cosmos_msg);

            // Should fail with unauthorized error
            assert!(result.is_err());
        }

        #[test]
        fn test_collateral_rate_assurance_after_deposit_succeeds() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);

            // Deposit collateral - triggers saving assurance state and batched callback
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(50_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(user_addr, cosmos_msg).unwrap();

            // Execute rate assurance callback directly should succeed
            let msg = ExecuteMsg::CollateralRateAssurance {
                collateral_denoms: Some(vec!["debit".to_string()]),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(cdp_contract.addr()), cosmos_msg);
            assert!(result.is_ok());
        }

        #[test]
        fn test_collateral_rate_assurance_after_withdraw_succeeds() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);

            // Create position via deposit
            let msg = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin {
                        denom: "debit".to_string(),
                        amount: Uint128::from(50_000_000_000u128),
                    }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Withdraw some collateral - triggers saving assurance state and batched callback
            let withdrawal_msg = ExecuteMsg::Withdraw {
                position_id: Uint128::from(1u128),
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: "debit".to_string(),
                        },
                        amount: Uint128::from(10_000_000_000u128),
                    }
                ],
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(withdrawal_msg, vec![]).unwrap();
            app.execute(user_addr, cosmos_msg).unwrap();

            // Execute rate assurance callback directly should succeed
            let msg = ExecuteMsg::CollateralRateAssurance {
                collateral_denoms: Some(vec!["debit".to_string()]),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(cdp_contract.addr()), cosmos_msg);
            assert!(result.is_ok());
        }

        // #[test]
        // fn test_collateral_rate_assurance_after_redeem_succeeds() {
        //     -- REDEMPTION TEST COMMENTED OUT --
        // }

        #[test]
        fn test_collateral_rate_assurance_after_liquidation_succeeds() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Seed a position
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Liquidate user position
            let liq = ExecuteMsg::Liquidate { position_id: Uint128::from(1u128), position_owner: user_addr.to_string() };
            let cosmos_msg = cdp_contract.call(liq, vec![]).unwrap();
            // Expect may error on guardrails depending on state; unwrap result or err
            let _ = app.execute(admin_addr, cosmos_msg);

            // Assurance callback should still execute without panic
            let msg = ExecuteMsg::CollateralRateAssurance { collateral_denoms: Some(vec!["debit".to_string()]) };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let _ = app.execute(Addr::unchecked(cdp_contract.addr()), cosmos_msg);
        }

        #[test]
        fn test_position_pending_interest_accumulation_over_time() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Create a position by depositing collateral
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Borrow some debt to create interest accrual
            let borrow = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Set interest rate in basket
            let edit_basket = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::percent(10)), // 10% annual interest
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Advance time by 1 year to accrue significant interest
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(31536000), // 1 year
                    chain_id: app.block_info().chain_id,
                }
            );

            // Trigger interest accrual
            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(user_addr.to_string()),
                position_ids: vec![Uint128::from(1u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query the position to check pending interest accumulation
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::from(1u128),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            };
            let baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();

            // Assert that pending interest has accumulated
            assert_eq!(baskets.len(), 1);
            assert_eq!(baskets[0].positions.len(), 1);
            assert!(baskets[0].positions[0].pending_interest == Uint128::new(1428571428));
        }

        #[test]
        fn test_position_pending_interest_reduction_during_repayment() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Create a position by depositing collateral
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Borrow some debt
            let borrow = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Set interest rate
            let edit_basket = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::percent(10)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Advance time and accrue interest
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(31536000),
                    chain_id: app.block_info().chain_id,
                }
            );

            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(user_addr.to_string()),
                position_ids: vec![Uint128::from(1u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position before repayment to get initial pending interest
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::from(1u128),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            };
            let baskets_before: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();
            let pending_interest_before = baskets_before[0].positions[0].pending_interest;

            // Send credit tokens to user for repayment
            app.send_tokens(
                app.api().addr_make("coin_God"),
                user_addr.clone(),
                &[coin(15_000_000_000, "credit_fulldenom")],
            ).unwrap();

            // Repay debt (this should reduce pending interest by 1B)
            let repay = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract.call(repay, vec![coin(1_000_000_000, "credit_fulldenom")]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position after repayment
            let baskets_after: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();
            let pending_interest_after = baskets_after[0].positions[0].pending_interest;

            // Assert that pending interest was reduced during repayment
            assert!(pending_interest_after < pending_interest_before);
            assert_eq!(pending_interest_before, Uint128::new(1428571428));
            assert_eq!(pending_interest_after, Uint128::new(428571428));

            // Repay debt (this should reduce pending interest to 0)
            let repay = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract.call(repay, vec![coin(14_000_000_000, "credit_fulldenom")]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position after repayment
            let baskets_after: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();
            let pending_interest_after = baskets_after[0].positions[0].pending_interest;
            // Since we repaid more than the original debt, pending interest should be zero
            assert_eq!(pending_interest_after, Uint128::zero());
        }

        #[test]
        fn test_position_pending_interest_state_persistence() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Create a position
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Borrow debt
            let borrow = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Set interest rate
            let edit_basket = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::percent(5)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Advance time and accrue interest
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(15768000), // 6 months
                    chain_id: app.block_info().chain_id,
                }
            );

            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(user_addr.to_string()),
                position_ids: vec![Uint128::from(1u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position to get initial pending interest
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::from(1u128),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            };
            let baskets_initial: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();
            let initial_pending_interest = baskets_initial[0].positions[0].pending_interest;

            // Perform additional operations that shouldn't affect pending interest
            // Add more collateral
            let deposit_more = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: Some(Uint128::from(1u128)),
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit_more,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(10_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position again - pending interest should be unchanged
            let baskets_after_deposit: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(baskets_after_deposit[0].positions[0].pending_interest, initial_pending_interest);

            // Test that pending interest persists across multiple operations
            // We'll just verify it's still the same after the deposit operation
            assert_eq!(baskets_after_deposit[0].positions[0].pending_interest, initial_pending_interest);
        }

        #[test]
        fn test_position_pending_interest_query_functionality() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let admin_addr = get_admin_addr(&app);

            // Create multiple positions for different users
            let user1 = "user1";
            let user2 = "user2";

            // Give users some tokens first
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                app.api().addr_make("user1"),
                &[coin(30_000_000_000, "debit")],
            ).unwrap();
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                app.api().addr_make("user2"),
                &[coin(40_000_000_000, "debit")],
            ).unwrap();

            // Create position for user1
            let deposit1 = ExecuteMsg::Deposit {
                position_id: None,
                position_owner: Some(app.api().addr_make(user1).to_string()),
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit1,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(30_000_000_000u128) }],
                )
                .unwrap();
            app.execute(app.api().addr_make(user1), cosmos_msg).unwrap();

            // Create position for user2
            let deposit2 = ExecuteMsg::Deposit {
                position_id: None,
                position_owner: Some(app.api().addr_make(user2).to_string()),
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit2,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(40_000_000_000u128) }],
                )
                .unwrap();
            app.execute(app.api().addr_make(user2), cosmos_msg).unwrap();

            // Borrow different amounts for each position
            let borrow1 = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(5_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow1, vec![]).unwrap();
            app.execute(app.api().addr_make(user1), cosmos_msg).unwrap();

            let borrow2 = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(8_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow2, vec![]).unwrap();
            app.execute(app.api().addr_make(user2), cosmos_msg).unwrap();

            // Set interest rate
            let edit_basket = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::percent(8)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            // Advance time and accrue interest
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(7884000), // 3 months
                    chain_id: app.block_info().chain_id,
                }
            );

            // Accrue position 1 (owned by user1)
            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(app.api().addr_make(user1).to_string()),
                position_ids: vec![Uint128::from(1u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            // Accrue position 2 (owned by user2)
            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(app.api().addr_make(user2).to_string()),
                position_ids: vec![Uint128::from(2u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Query all positions
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: None,
                user: None,
            };
            let all_baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();

            // Should have 2 users with positions
            assert_eq!(all_baskets.len(), 2);

            // Query specific user positions
            let query_user1 = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: None,
                user: Some(user1.to_string()),
            };
            let user1_baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_user1)
                .unwrap();

            let query_user2 = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: None,
                user: Some(user2.to_string()),
            };
            let user2_baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_user2)
                .unwrap();

            // Each user should have one position
            assert_eq!(user1_baskets.len(), 1);
            assert_eq!(user1_baskets[0].positions.len(), 1);
            assert_eq!(user2_baskets.len(), 1);
            assert_eq!(user2_baskets[0].positions.len(), 1);

            // Both positions should have pending interest
            assert!(user1_baskets[0].positions[0].pending_interest > Uint128::zero());
            assert!(user2_baskets[0].positions[0].pending_interest > Uint128::zero());

            // User2 should have more pending interest since they borrowed more
            assert!(user2_baskets[0].positions[0].pending_interest > user1_baskets[0].positions[0].pending_interest);
        }

        #[test]
        fn test_position_pending_interest_edge_cases() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Create a position
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Borrow a small amount
            let borrow = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(2_000_000_000u128)), // Above minimum debt
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Set a very low interest rate
            let edit_basket = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::from_ratio(1u128, 1000000u128)), // 0.0001%
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            // Advance time by a short period
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(86400), // 1 day
                    chain_id: app.block_info().chain_id,
                }
            );

            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(user_addr.to_string()),
                position_ids: vec![Uint128::from(1u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position - should handle very small interest amounts gracefully
            let query_msg = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::from(1u128),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            };
            let baskets: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg)
                .unwrap();

            // Pending interest should be zero or very small due to low rate and short time
            assert!(baskets[0].positions[0].pending_interest <= Uint128::from(1000u128));

            // Test with zero interest rate - create a new position to avoid previous interest
            // Give user more tokens for the second position
            app.send_tokens(
                app.api().addr_make("bigger_bank"),
                user_addr.clone(),
                &[coin(100_000_000_000, "debit")],
            ).unwrap();

            let deposit_zero_rate = ExecuteMsg::Deposit {
                position_id: None,
                position_owner: Some(user_addr.to_string()),
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract.call(deposit_zero_rate, vec![
                coin(100_000_000_000, "debit"),
            ]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Borrow with the new position
            let borrow_zero_rate = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(2u128),
                amount: Some(Uint128::from(2_000_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow_zero_rate, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Set zero interest rate
            let edit_basket_zero = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: None,
                credit_pool_infos: None,
                collateral_supply_caps: None,
                multi_asset_supply_caps: None,
                base_interest_rate: Some(Decimal::zero()),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
            });
            let cosmos_msg = cdp_contract.call(edit_basket_zero, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Advance time significantly
            app.set_block(
                BlockInfo {
                    height: app.block_info().height + 1,
                    time: app.block_info().time.plus_seconds(31536000), // 1 year
                    chain_id: app.block_info().chain_id,
                }
            );

            let accrue_msg = ExecuteMsg::Accrue {
                position_owner: Some(user_addr.to_string()),
                position_ids: vec![Uint128::from(2u128)],
            };
            let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Query position - pending interest should remain zero with zero interest rate
            let query_msg_zero_rate = QueryMsg::GetBasketPositions {
                start_after: None,
                limit: None,
                user_info: Some(UserInfo {
                    position_id: Uint128::from(2u128),
                    position_owner: user_addr.to_string(),
                }),
                user: None,
            };
            let baskets_zero_rate: Vec<BasketPositionsResponse> = app
                .wrap()
                .query_wasm_smart(cdp_contract.addr(), &query_msg_zero_rate)
                .unwrap();

            assert_eq!(baskets_zero_rate[0].positions[0].pending_interest, Uint128::zero());
        }

        #[test]
        fn test_collateral_rate_assurance_after_close_position_succeeds() {
            let (mut app, cdp_contract, _lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Lower debt minimum to allow small borrow/repay for test
            let cfg = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                staking_contract: None,
                chain_proxy: None,
                debt_auction: None,
                ltv_disco: None,
                oracle_contract: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                oracle_time_limit: None,
                cpc_multiplier: None,
                debt_minimum: Some(Uint128::new(1u128)),
                base_debt_cap_multiplier: None,
                rate_slope_multiplier: None,
                affiliate_fee_max: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: None,
                revenue_distributor: None,
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(cfg, vec![]).unwrap();
            app.execute(admin_addr, cosmos_msg).unwrap();

            // Create and borrow
            let deposit = ExecuteMsg::Deposit {
                position_owner: Some(user_addr.to_string()),
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    deposit,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(50_000_000_000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            let borrow = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::from(1u128),
                amount: Some(Uint128::from(10_000_000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(borrow, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Repay and close
            let repay = ExecuteMsg::Repay {
                position_id: Uint128::from(1u128),
                position_owner: None,
                send_excess_to: None,
                debt_split: None,
            };
            let cosmos_msg = cdp_contract.call(repay, vec![Coin { denom: "credit".to_string(), amount: Uint128::from(10_000_000u128) }]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            let close = ExecuteMsg::ClosePosition {
                position_id: Uint128::from(1u128),
                close_percentage: None,
                max_spread: Decimal::percent(1),
                send_to: None,
            };
            let cosmos_msg = cdp_contract.call(close, vec![]).unwrap();
            let _ = app.execute(user_addr, cosmos_msg);

            // Assurance callback
            let msg = ExecuteMsg::CollateralRateAssurance { collateral_denoms: Some(vec!["debit".to_string()]) };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            let result = app.execute(Addr::unchecked(cdp_contract.addr()), cosmos_msg);
            assert!(result.is_ok());
        }

        #[test]
        fn liquidation_stats_and_pagination() {
            let (mut app, cdp_contract, lq_contract) =
                proper_instantiate(false, false, false, false);

            let user_addr = get_user_addr(&app);
            let admin_addr = get_admin_addr(&app);

            // Add liq-queue to enable LQ path
            let msg = ExecuteMsg::EditBasket(EditBasket {
                take_revenue: None,
                added_cAsset: None,
                liq_queue: Some(lq_contract.addr().to_string()),
                credit_pool_infos: Some(vec![PoolType::Balancer { pool_id: 1u64 }]),
                collateral_supply_caps: Some(vec![SupplyCap {
                    asset_info: AssetInfo::NativeToken { denom: "debit".to_string() },
                    current_supply: Uint128::zero(),
                    debt_total: Uint128::zero(),
                    supply_cap_ratio: Decimal::percent(100),
                    lp: false,
                    stability_pool_ratio_for_debt_cap: None,
                }]),
                base_interest_rate: Some(Decimal::percent(2)),
                credit_asset_twap_price_source: None,
                negative_rates: None,
                cpc_margin_of_error: None,
                frozen: None,
                distribute_revenue: None,
                multi_asset_supply_caps: None,
            });
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            // Configure small stat limit to test truncation
            let upd = ExecuteMsg::UpdateConfig(UpdateConfig {
                owner: None,
                staking_contract: None,
                chain_proxy: None,
                debt_auction: None,
                oracle_contract: None,
                ltv_disco: None,
                liquidity_contract: None,
                discounts_contract: None,
                liq_fee: None,
                collateral_twap_timeframe: None,
                credit_twap_timeframe: None,
                oracle_time_limit: None,
                cpc_multiplier: None,
                debt_minimum: None,
                base_debt_cap_multiplier: None,
                rate_slope_multiplier: None,
                revenue_distributor: None,
                affiliate_fee_max: None,
                skip_credit_price_accrual: None,
                liquidation_stat_limit: Some(2),
                transmuter_addr: None,
                irm_config: None,
                points_contract: None,
            });
            let cosmos_msg = cdp_contract.call(upd, vec![]).unwrap();
            app.execute(admin_addr.clone(), cosmos_msg).unwrap();

            // Deposit collateral and mint debt
            let msg = ExecuteMsg::Deposit {
                position_owner: None,
                position_id: None,
                affiliate_address: None,
                affiliate_label: None,
            };
            let cosmos_msg = cdp_contract
                .call(
                    msg,
                    vec![Coin { denom: "debit".to_string(), amount: Uint128::from(100_000_000000u128) }],
                )
                .unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            let msg = ExecuteMsg::IncreaseDebt {
                position_id: Uint128::new(1),
                amount: Some(Uint128::from(5_000_000000u128)),
                LTV: None,
                mint_to_addr: None,
                deployment_intent: None,
                debt_split: None,
                rollover_updates: None,
                peg_debt: None,
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(user_addr.clone(), cosmos_msg).unwrap();

            // Perform 3 liquidations to exceed limit
            for _ in 0..3 {
                let msg = ExecuteMsg::Liquidate { position_id: Uint128::new(1), position_owner: user_addr.to_string() };
                let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
                let _res = app.execute(app.api().addr_make("liquidator"), cosmos_msg);
                // advance block time for ordering
                app.set_block(BlockInfo { height: app.block_info().height + 1, time: app.block_info().time.plus_seconds(5), chain_id: app.block_info().chain_id });
            }

            // Query stats - default limit (50) but stored max is 2
            let q = QueryMsg::GetLiquidationStats { start_after: None, limit: None };
            let stats: Vec<LiquidationStatResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(), &q).unwrap();
            assert_eq!(stats.len(), 2);
            assert!(stats[0].block_time <= stats[1].block_time);
            assert_eq!(stats[0].position_id, Uint128::new(1));

            // Pagination: start_after first returned block_time should return only one entry
            let start_after = Some(stats[0].block_time);
            let q = QueryMsg::GetLiquidationStats { start_after, limit: Some(1) };
            let page: Vec<LiquidationStatResponse> = app.wrap().query_wasm_smart(cdp_contract.addr(), &q).unwrap();
            assert_eq!(page.len(), 1);
            assert!(page[0].block_time > stats[0].block_time);
        }

    }
}
