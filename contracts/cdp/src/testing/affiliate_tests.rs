pub mod tests {

    mod cdp {
        use super::*;
    }

    use std::str::FromStr;

    use crate::helpers::{CDPContract, LQContract};

    use membrane::cdp::{
        ExecuteMsg, QueryMsg, EditBasket, UpdateConfig, Config, BasketPositionsResponse,
    };
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, TWAPPoolInfo, UserInfo, PoolType, SupplyCap,
        Basket,
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
    fn test_set_affiliate_valid() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set affiliate for position 1
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let result = app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Check that the affiliate was set successfully
        assert!(result.events.iter().any(|event| {
            event.attributes.iter().any(|attr| {
                attr.key == "method" && attr.value == "set_affiliate"
            })
        }));

        // Query affiliates to verify
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(1),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 1);
        assert_eq!(affiliates[0].affiliate_address, app.api().addr_make("affiliate1").as_str());
        assert_eq!(affiliates[0].affiliate_fee, Decimal::percent(5));
    }

    #[test]
    fn test_set_affiliate_fee_validation() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Test zero fee - should fail
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::zero(),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

        // Test fee exceeding max (10%) - should fail
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(15),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

        // Test valid fee - should succeed
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(8),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();
    }

    #[test]
    fn test_set_affiliate_limit() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Add 3 affiliates - should all succeed
        for i in 1..=3 {
            let msg = ExecuteMsg::SetAffiliate {
                position_id: Uint128::new(1),
                affiliate_address: format!("affiliate{}", i),
                affiliate_fee: Decimal::percent(3),
                label: Some(format!("test_affiliate_{}", i)),
            };
            let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
            app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();
        }

        // Try to add a 4th affiliate - should fail
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: "affiliate4".to_string(),
            affiliate_fee: Decimal::percent(3),
            label: Some("test_affiliate_4".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap_err();

        // Verify we have exactly 3 affiliates
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(1),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 3);
    }

    #[test]
    fn test_update_affiliate_fee_by_affiliate() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set initial affiliate
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Update fee by the affiliate themselves - should succeed
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(7),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        let result = app
            .execute(app.api().addr_make("affiliate1"), cosmos_msg)
            .unwrap();

        // Check that the fee was updated
        assert!(result.events.iter().any(|event| {
            event.attributes.iter().any(|attr| {
                attr.key == "affiliate_fee_updated" && attr.value == "0.07"
            })
        }));

        // Verify the updated fee
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(1),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 1);
        assert_eq!(affiliates[0].affiliate_fee, Decimal::percent(7));
    }

    #[test]
    fn test_update_affiliate_fee_unauthorized() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set initial affiliate
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Try to update fee by a different user - should fail
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(7),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make("unauthorized_user"), cosmos_msg)
            .unwrap_err();

        // Verify the fee wasn't changed
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(1),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 1);
        assert_eq!(affiliates[0].affiliate_fee, Decimal::percent(5));
    }

    #[test]
    fn test_affiliate_fee_distribution_single() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, true, false);

        // Create a position first
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
                    amount: Uint128::from(100_000_000_000u128), // 100 debit tokens
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set affiliate for position 1
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time significantly to accrue interest and generate revenue
        app.set_block(BlockInfo {
            height: app.block_info().height,
            time: app.block_info().time.plus_seconds(31536000u64), // 1 year later
            chain_id: app.block_info().chain_id,
        });

        // Set up revenue destinations to enable affiliate fee distribution
        let res: Config = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &QueryMsg::Config {})
            .unwrap();
        let staking_contract = res.staking_contract.unwrap();

        let msg = ExecuteMsg::EditBasket(EditBasket {
            take_revenue: None,
            added_cAsset: None,
            liq_queue: Some(_lq_contract.addr().to_string()),
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
            base_interest_rate: Some(Decimal::percent(10)), // Set 10% annual interest rate
            credit_asset_twap_price_source: None,
            negative_rates: None,
            cpc_margin_of_error: None,
            frozen: None,
            distribute_revenue: None,
            multi_asset_supply_caps: None,
        });
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(ADMIN), cosmos_msg).unwrap();

        // Increase debt to simulate revenue generation
        let increase_debt_msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::from(49_999_000_000u128)), // Almost 50 billion CDT
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time 1 more year AFTER debt exists for interest to accrue
        app.set_block(BlockInfo {
            height: app.block_info().height,
            time: app.block_info().time.plus_seconds(31536000u64), // 1 more year for interest to accrue
            chain_id: app.block_info().chain_id,
        });

        // Accrue interest to generate pending revenue
        let accrue_msg = ExecuteMsg::Accrue {
            position_owner: None,
            position_ids: vec![Uint128::new(1)],
        };
        let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Send credit tokens to user for repayment
        app.send_tokens(
            app.api().addr_make("sender"),
            app.api().addr_make(USER),
            &[coin(49_999_000_000, "credit_fulldenom")],
        )
        .unwrap();

        // Check basket state to see if there's pending revenue
        let basket_query = QueryMsg::GetBasket {};
        let basket: Basket = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &basket_query)
            .unwrap();
        println!("Pending revenue: {}", basket.pending_revenue);
        println!("Distribute revenue: {}", basket.distribute_revenue);

        // Check affiliate balance before repayment
        let affiliate_balance_before = app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();

        // Repay some debt to trigger affiliate fee distribution
        let repay_msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: None,
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract
            .call(
                repay_msg,
                vec![Coin {
                    denom: "credit_fulldenom".to_string(),
                    amount: Uint128::from(25_000_000_000u128), // 25 billion CDT - leave more debt
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Check affiliate balance after repayment
        let affiliate_balance_after = app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();

        // Verify affiliate received fee
        let fee_received = affiliate_balance_after.amount - affiliate_balance_before.amount;
        println!(
            "Affiliate balance before: {}",
            affiliate_balance_before.amount
        );
        println!(
            "Affiliate balance after: {}",
            affiliate_balance_after.amount
        );
        println!("Fee received: {}", fee_received);
        assert!(
            fee_received > Uint128::zero(),
            "Affiliate should have received a fee"
        );
    }

    #[test]
    fn test_affiliate_fee_distribution_multiple() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set first affiliate
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(3),
            label: Some("test_affiliate_1".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1800); // 30 minutes later
        });

        // Set second affiliate
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate2").to_string(),
            affiliate_fee: Decimal::percent(4),
            label: Some("test_affiliate_2".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1800); // Another 30 minutes later
        });

        // Set third affiliate
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate3").to_string(),
            affiliate_fee: Decimal::percent(2),
            label: Some("test_affiliate_3".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1800); // Another 30 minutes later
        });

        // Increase debt to simulate revenue generation
        let increase_debt_msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::from(20_000_000_000u128)), // 20 CDT
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time 1 year AFTER debt exists for interest to accrue
        app.set_block(BlockInfo {
            height: app.block_info().height,
            time: app.block_info().time.plus_seconds(31536000u64), // 1 more year for interest to accrue
            chain_id: app.block_info().chain_id,
        });

        // Accrue interest to generate pending revenue
        let accrue_msg = ExecuteMsg::Accrue {
            position_owner: None,
            position_ids: vec![Uint128::new(1)],
        };
        let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Send credit tokens to user for repayment
        app.send_tokens(
            app.api().addr_make("sender"),
            app.api().addr_make(USER),
            &[coin(20_000_000_000, "credit_fulldenom")],
        )
        .unwrap();

        // Check affiliate balances before repayment
        let affiliate1_balance_before =
            app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();
        let affiliate2_balance_before =
            app.wrap().query_balance(&app.api().addr_make("affiliate2").to_string(), "debit").unwrap();
        let affiliate3_balance_before =
            app.wrap().query_balance(&app.api().addr_make("affiliate3").to_string(), "debit").unwrap();

        // Repay some debt to trigger affiliate fee distribution
        let repay_msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: None,
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract
            .call(
                repay_msg,
                vec![Coin {
                    denom: "credit_fulldenom".to_string(),
                    amount: Uint128::from(10_000_000_000u128), // 10 CDT
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Check affiliate balances after repayment
        let affiliate1_balance_after =
            app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();
        let affiliate2_balance_after =
            app.wrap().query_balance(&app.api().addr_make("affiliate2").to_string(), "debit").unwrap();
        let affiliate3_balance_after =
            app.wrap().query_balance(&app.api().addr_make("affiliate3").to_string(), "debit").unwrap();

        // Verify all affiliates received fees
        let fee1_received =
            affiliate1_balance_after.amount - affiliate1_balance_before.amount;
        let fee2_received =
            affiliate2_balance_after.amount - affiliate2_balance_before.amount;
        let fee3_received =
            affiliate3_balance_after.amount - affiliate3_balance_before.amount;

        assert!(
            fee1_received > Uint128::zero(),
            "Affiliate1 should have received a fee"
        );
        assert!(
            fee2_received > Uint128::zero(),
            "Affiliate2 should have received a fee"
        );
        assert!(
            fee3_received > Uint128::zero(),
            "Affiliate3 should have received a fee"
        );

        // Verify that affiliate1 received more than affiliate3 (longer time affiliated)
        assert!(
            fee1_received > fee3_received,
            "Affiliate1 should receive more than affiliate3 due to longer affiliation time"
        );
    }

    #[test]
    fn test_affiliate_fee_distribution_time_based() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set first affiliate (will be affiliated for longer)
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: "long_term_affiliate".to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("long_term_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time significantly
        app.update_block(|block| {
            block.time = block.time.plus_seconds(7200); // 2 hours later
        });

        // Set second affiliate (will be affiliated for shorter time)
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: "short_term_affiliate".to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("short_term_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time slightly
        app.update_block(|block| {
            block.time = block.time.plus_seconds(1800); // 30 minutes later
        });

        // Increase debt to simulate revenue generation
        let increase_debt_msg = ExecuteMsg::IncreaseDebt {
            position_id: Uint128::new(1),
            amount: Some(Uint128::from(15_000_000_000u128)), // 15 CDT
            LTV: None,
            mint_to_addr: None,
            deployment_intent: None,
            debt_split: None,
            rollover_updates: None,
            peg_debt: None,
        };
        let cosmos_msg = cdp_contract.call(increase_debt_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time 1 year AFTER debt exists for interest to accrue
        app.set_block(BlockInfo {
            height: app.block_info().height,
            time: app.block_info().time.plus_seconds(31536000u64), // 1 more year for interest to accrue
            chain_id: app.block_info().chain_id,
        });

        // Accrue interest to generate pending revenue
        let accrue_msg = ExecuteMsg::Accrue {
            position_owner: None,
            position_ids: vec![Uint128::new(1)],
        };
        let cosmos_msg = cdp_contract.call(accrue_msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Send credit tokens to user for repayment
        app.send_tokens(
            app.api().addr_make("sender"),
            app.api().addr_make(USER),
            &[coin(15_000_000_000, "credit_fulldenom")],
        )
        .unwrap();

        // Check affiliate balances before repayment
        let long_term_balance_before = app
            .wrap()
            .query_balance("long_term_affiliate", "debit")
            .unwrap();
        let short_term_balance_before = app
            .wrap()
            .query_balance("short_term_affiliate", "debit")
            .unwrap();

        // Repay some debt to trigger affiliate fee distribution
        let repay_msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: None,
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract
            .call(
                repay_msg,
                vec![Coin {
                    denom: "credit_fulldenom".to_string(),
                    amount: Uint128::from(8_000_000_000u128), // 8 CDT
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Check affiliate balances after repayment
        let long_term_balance_after = app
            .wrap()
            .query_balance("long_term_affiliate", "debit")
            .unwrap();
        let short_term_balance_after = app
            .wrap()
            .query_balance("short_term_affiliate", "debit")
            .unwrap();

        // Verify both affiliates received fees
        let long_term_fee =
            long_term_balance_after.amount - long_term_balance_before.amount;
        let short_term_fee =
            short_term_balance_after.amount - short_term_balance_before.amount;

        assert!(
            long_term_fee > Uint128::zero(),
            "Long-term affiliate should have received a fee"
        );
        assert!(
            short_term_fee > Uint128::zero(),
            "Short-term affiliate should have received a fee"
        );

        // Verify that long-term affiliate received significantly more than short-term affiliate
        // Long-term affiliate was affiliated for 2 hours, short-term for 30 minutes
        // So long-term should receive about 4x more (2 hours / 30 minutes = 4)
        assert!(
            long_term_fee > short_term_fee,
            "Long-term affiliate should receive more than short-term affiliate"
        );

        // The ratio should be approximately 4:1 (2 hours vs 30 minutes)
        let ratio = long_term_fee / short_term_fee;
        assert!(
            ratio >= Uint128::from(3u128),
            "Long-term affiliate should receive at least 3x more than short-term affiliate"
        );
    }

    #[test]
    fn test_affiliate_fee_distribution_no_revenue() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Set affiliate for position 1
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(5),
            label: Some("test_affiliate".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Advance time
        app.update_block(|block| {
            block.time = block.time.plus_seconds(3600); // 1 hour later
        });

        // Send credit tokens to user for repayment
        app.send_tokens(
            app.api().addr_make("sender"),
            app.api().addr_make(USER),
            &[coin(5_000_000_000, "credit_fulldenom")],
        )
        .unwrap();

        // Check affiliate balance before repayment
        let affiliate_balance_before =
            app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();

        // Repay debt without generating revenue (repay more than minted)
        let repay_msg = ExecuteMsg::Repay {
            position_id: Uint128::new(1),
            position_owner: None,
            send_excess_to: None,
            debt_split: None,
        };
        let cosmos_msg = cdp_contract
            .call(
                repay_msg,
                vec![Coin {
                    denom: "credit_fulldenom".to_string(),
                    amount: Uint128::from(1_000_000_000u128), // 1 CDT
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Check affiliate balance after repayment
        let affiliate_balance_after =
            app.wrap().query_balance(&app.api().addr_make("affiliate1").to_string(), "debit").unwrap();

        // Verify affiliate received no fee when there's no revenue
        let fee_received =
            affiliate_balance_after.amount - affiliate_balance_before.amount;
        assert_eq!(
            fee_received,
            Uint128::zero(),
            "Affiliate should not receive fee when there's no revenue"
        );
    }

    #[test]
    fn test_query_affiliates() {
        let (mut app, cdp_contract, _lq_contract) =
            proper_instantiate(false, false, false, false);

        // Create a position first
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
                    amount: Uint128::from(50_000_000_000u128),
                }],
            )
            .unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Query affiliates for non-existent position - should return empty
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(999),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 0);

        // Add some affiliates
        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate1").to_string(),
            affiliate_fee: Decimal::percent(3),
            label: Some("test_affiliate_1".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        let msg = ExecuteMsg::SetAffiliate {
            position_id: Uint128::new(1),
            affiliate_address: app.api().addr_make("affiliate2").to_string(),
            affiliate_fee: Decimal::percent(4),
            label: Some("test_affiliate_2".to_string()),
        };
        let cosmos_msg = cdp_contract.call(msg, vec![]).unwrap();
        app.execute(app.api().addr_make(USER), cosmos_msg).unwrap();

        // Query affiliates for position 1 - should return 2 affiliates
        let query_msg = QueryMsg::GetAffiliates {
            position_id: Uint128::new(1),
        };
        let affiliates: Vec<membrane::types::AffiliateData> = app
            .wrap()
            .query_wasm_smart(cdp_contract.addr(), &query_msg)
            .unwrap();

        assert_eq!(affiliates.len(), 2);

        // Check that both affiliates are present with correct data
        let affiliate1 = affiliates
            .iter()
            .find(|a| a.affiliate_address == app.api().addr_make("affiliate1").to_string())
            .unwrap();
        assert_eq!(affiliate1.affiliate_fee, Decimal::percent(3));

        let affiliate2 = affiliates
            .iter()
            .find(|a| a.affiliate_address == "affiliate2")
            .unwrap();
        assert_eq!(affiliate2.affiliate_fee, Decimal::percent(4));
    }
}
