


#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier, MockStorage},
        from_binary, coins, Addr, Decimal, Uint128, Coin, StdError, StdResult,
    };
    use crate::contract::{instantiate, execute, query};
    use membrane::{managed_market::{BorrowCap, CollateralParams, Config, ExecuteMsg, InstantiateMsg, QueryMsg, RateParams, UserPositionResponse}, types::{AssetOracleInfo, TWAPPoolInfo, UserPosition}};
    use crate::state::{CONFIG, POSITIONS};


        pub const CDT_DENOM: &str = "factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt";

    //Contract state tests:
    // - Rate Accrual stays up to date during withdraws/borrow/repay
    // - Vault token updates
    // - Pause doesn't work if not activated at instantiation

    fn default_instantiate_msg() -> InstantiateMsg {
        InstantiateMsg {
            owner: "owner".to_string(),
            osmosis_proxy_contract: "proxy".to_string(),
            whitelisted_debt_suppliers:  Some(vec!["debt_guy".to_string()]),
            debt_supply_vault_token: "vault_token".to_string(),
            collateral_params: CollateralParams {
                collateral_asset: "atom".to_string(),
                max_borrow_LTV: Decimal::percent(50),
                liquidation_LTV: Decimal::percent(60),
            },
            rate_params: RateParams {
                base_rate: Decimal::percent(5),
                rate_kink: None,
                rate_max: Decimal::percent(20),
            },
            pool_for_oracle_and_liquidations: AssetOracleInfo {
                basket_id: Uint128::one(),
                pyth_price_feed_id: None,
                pools_for_osmo_twap: vec![
                    TWAPPoolInfo {
                        pool_id: 1u64,
                        base_asset_denom: "atom".to_string(),
                        quote_asset_denom: "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4".to_string(),
                    }
                ],
                is_usd_par: false,
                lp_pool_info: None,
                vault_info: None,
                decimals: 6
            },
            borrow_fee: Decimal::percent(1),
            whitelisted_collateral_suppliers: Some(vec!["collateral_guy".to_string()]),
            pause_option: true,
            debt_supply_cap: None,
            borrow_cap: BorrowCap {
                fixed_cap: Some(Uint128::new(1_000_000)),
                cap_borrows_by_liquidity: false,
            },
        }
    }

    #[test]
    fn test_supply_collateral_happy_path_and_failures() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        // Instantiate contract
        let msg = default_instantiate_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Happy Path: Successful collateral supply
        let deposit_info = mock_info("collateral_guy", &[Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(1_000_000),
        }]);

        let msg = ExecuteMsg::SupplyCollateral { owner: None };
        let res = execute(deps.as_mut(), env.clone(), deposit_info.clone(), msg).unwrap();
        assert_eq!(res.messages.len(), 0);

        // Check state updated
        let value: Vec<UserPositionResponse> =
            from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::GetUserPositions { 
                collateral_denom: String::from("atom"), 
                user: Some("collateral_guy".to_string()), 
                start_after: None, 
                limit: None 
            }).unwrap()).unwrap();

        assert_eq!(
            value,
            vec![UserPositionResponse {
                user: "collateral_guy".to_string(),
                position: UserPosition {
                    collateral_denom: "atom".to_string(),
                    collateral_amount: Uint128::new(1_000_000),
                    debt_amount: Uint128::zero(),
                    rate_index: Decimal::one(),
                }
            }]
        );

        // Failure: Multiple assets sent
        let multi_asset_info = mock_info("collateral_guy", &[
            Coin { denom: "atom".to_string(), amount: Uint128::new(500_000) },
            Coin { denom: "usdc".to_string(), amount: Uint128::new(500_000) },
        ]);
        let msg = ExecuteMsg::SupplyCollateral { owner: None };
        let err = execute(deps.as_mut(), env.clone(), multi_asset_info, msg.clone()).unwrap_err();
        assert_eq!(err.to_string(),  "Custom Error val: \"Need to send one collateral asset only\"".to_string());

        // Failure: Market does not exist
        let invalid_denom_info = mock_info("collateral_guy", &[Coin {
            denom: "nonexistent".to_string(),
            amount: Uint128::new(100),
        }]);
        let err = execute(deps.as_mut(), env.clone(), invalid_denom_info, msg.clone()).unwrap_err();
        assert_eq!(err.to_string(), "Custom Error val: \"Collateral asset (\\\"nonexistent\\\") not supported\"".to_string());

        // Failure: Contract paused
        //Update to pause the contract
        let update_msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            osmosis_proxy_contract_addr: None, 
            pause_actions: Some(true),
            manager_fee: None, 
            whitelisted_debt_suppliers: None, 
            debt_supply_cap: None
        };
        let admin_info = mock_info("owner", &[]);
        let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), update_msg).unwrap();
        //Attempt to deposit during paused state
        let paused_info = mock_info("collateral_guy", &[Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(500),
        }]);
        let err = execute(deps.as_mut(), env.clone(), paused_info, msg.clone()).unwrap_err();
        assert_eq!(err.to_string(), "Basket withdrawals & debt increases are frozen temporarily".to_string());

        // Unpause to continue
        let update_msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            osmosis_proxy_contract_addr: None, 
            pause_actions: Some(false),
            manager_fee: None, 
            whitelisted_debt_suppliers: None, 
            debt_supply_cap: None
        };
        let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), update_msg).unwrap();
        // Failure: Sender not whitelisted
        let unwhitelisted_info = mock_info("random_guy", &[Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        }]);
        let err = execute(deps.as_mut(), env.clone(), unwhitelisted_info, msg.clone()).unwrap_err();
        assert_eq!(err.to_string(), "Unauthorized, owner is owner".to_string());
    }

    #[test]
fn test_withdraw_collateral_happy_path_and_failures() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let msg = default_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Supply collateral first
    let deposit_info = mock_info("collateral_guy", &[Coin {
        denom: "atom".to_string(),
        amount: Uint128::new(1_000_000),
    }]);
    let msg = ExecuteMsg::SupplyCollateral { owner: None };
    execute(deps.as_mut(), env.clone(), deposit_info.clone(), msg).unwrap();

    // Happy path: Withdraw some collateral
    let withdraw_msg = ExecuteMsg::WithdrawCollateral {
        collateral_denom: "atom".to_string(),
        send_to: None,
        withdraw_amount: Some(Uint128::new(400_000)),
    };
    let withdraw_info = mock_info("collateral_guy", &[]);
    execute(deps.as_mut(), env.clone(), withdraw_info.clone(), withdraw_msg.clone()).unwrap();

    // Query and check user position
    let value: Vec<UserPositionResponse> =
        from_binary(&query(deps.as_ref(), mock_env(), QueryMsg::GetUserPositions {
            collateral_denom: "atom".to_string(),
            user: Some("collateral_guy".to_string()),
            start_after: None,
            limit: None,
        }).unwrap()).unwrap();
    assert_eq!(
        value,
        vec![UserPositionResponse {
            user: "collateral_guy".to_string(),
            position: UserPosition {
                collateral_denom: "atom".to_string(),
                collateral_amount: Uint128::new(600_000),
                debt_amount: Uint128::zero(),
                rate_index: Decimal::one(),
            }
        }]
    );

    // Failure: Withdraw with no position
    let msg = ExecuteMsg::WithdrawCollateral {
        collateral_denom: "atom".to_string(),
        send_to: None,
        withdraw_amount: Some(Uint128::new(100)),
    };
    let no_pos_info = mock_info("random_guy", &[]);
    let err = execute(deps.as_mut(), env.clone(), no_pos_info, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "membrane::types::UserPosition not found".to_string());

    // Success: Withdrawing more than available withdraws the max
    let msg = ExecuteMsg::WithdrawCollateral {
        collateral_denom: "atom".to_string(),
        send_to: Some(String::from("rando")),
        withdraw_amount: Some(Uint128::new(2_000_000)),
    };
    let res = execute(deps.as_mut(), env.clone(), withdraw_info.clone(), msg.clone()).unwrap();

    assert_eq!(format!("{:?}", res.messages), 
        "[SubMsg { id: 0, msg: Bank(Send { to_address: \"rando\", amount: [Coin { 600000 \"atom\" }] }), gas_limit: None, reply_on: Never }]"
    );

    // Failure: Contract paused
    let pause_msg = ExecuteMsg::UpdateConfig {
        owner: None,
        osmosis_proxy_contract_addr: None,
        pause_actions: Some(true),
        manager_fee: None,
        whitelisted_debt_suppliers: None,
        debt_supply_cap: None,
    };
    execute(deps.as_mut(), env.clone(), info.clone(), pause_msg).unwrap();

    let msg = ExecuteMsg::WithdrawCollateral {
        collateral_denom: "atom".to_string(),
        send_to: None,
        withdraw_amount: Some(Uint128::new(100)),
    };
    let err = execute(deps.as_mut(), env.clone(), withdraw_info.clone(), msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "Basket withdrawals & debt increases are frozen temporarily".to_string());
}

#[test]
fn test_supply_debt_happy_path_and_failures() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let msg = default_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Happy Path: Supply valid debt token
    let supplier = "debt_guy";
    let deposit_info = mock_info(supplier, &[Coin {
        denom: CDT_DENOM.to_string(),
        amount: Uint128::new(1_000_000),
    }]);
    let msg = ExecuteMsg::SupplyDebt { send_to: None };
    execute(deps.as_mut(), env.clone(), deposit_info.clone(), msg.clone()).unwrap();

    // Check config: total_debt_tokens updated
    let config: Config = from_binary(
        &query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap()
    ).unwrap();
    assert_eq!(config.total_debt_tokens, Uint128::new(1_000_000));
 
    // Failure: Multiple assets sent
    let multi = mock_info(supplier, &[
        Coin::new(500_000, CDT_DENOM),
        Coin::new(500_000, "other"),
    ]);
    let err = execute(deps.as_mut(), env.clone(), multi, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "Custom Error val: \"Need to send the debt asset only: factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt\"".to_string());

    // Failure: Zero asset sent
    let zero = mock_info(supplier, &[Coin::new(0, CDT_DENOM)]);
    let err = execute(deps.as_mut(), env.clone(), zero, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "amount was zero, must be positive".to_string());

    // Failure: Not the correct asset denom
    let wrong_denom = mock_info(supplier, &[Coin::new(1_000, "notusdc")]);
    let err = execute(deps.as_mut(), env.clone(), wrong_denom, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "Custom Error val: \"Need to send the debt asset only: factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt\"".to_string());

    // Failure: Not whitelisted
    let badguy = mock_info("unwhitelisted", &[Coin::new(1_000, CDT_DENOM)]);
    let err = execute(deps.as_mut(), env.clone(), badguy, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(),  "Custom Error val: \"Sender (\\\"unwhitelisted\\\") not whitelisted to supply debt\"".to_string());

    // Failure: Supply cap exceeded
    let update_msg = ExecuteMsg::UpdateConfig {
        owner: None,
        osmosis_proxy_contract_addr: None,
        pause_actions: None,
        manager_fee: None,
        whitelisted_debt_suppliers: None,
        debt_supply_cap: Some(Some(Uint128::new(1_000_001))), // Just 1 more
    };
    execute(deps.as_mut(), env.clone(), info.clone(), update_msg).unwrap();

    let too_much = mock_info(supplier, &[Coin::new(2, CDT_DENOM)]);
    let err = execute(deps.as_mut(), env.clone(), too_much, msg).unwrap_err();
    assert_eq!(err.to_string(), "Supply cap exceeded: 1000002 > 1000001".to_string());
}

#[test]
fn test_withdraw_debt_happy_path_and_failures() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin_info = mock_info("owner", &[]);

    // Instantiate
    let msg = default_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg).unwrap();


    // Happy Path: Supply valid debt token
    let supplier = "debt_guy";
    let deposit_info: cosmwasm_std::MessageInfo = mock_info(supplier, &[Coin {
        denom: CDT_DENOM.to_string(),
        amount: Uint128::new(1_000_000),
    }]);
    let msg = ExecuteMsg::SupplyDebt { send_to: None };
    execute(deps.as_mut(), env.clone(), deposit_info.clone(), msg.clone()).unwrap();

    //Give the contract some debt tokens
    deps.querier.update_balance(
        "cosmos2contract".clone(),
        vec![Coin {
            denom: CDT_DENOM.to_string(),
            amount: Uint128::new(1_000_000),
        }],
    );

    // Happy Path: Withdraw debt token
    let withdraw_info = mock_info(supplier, &[Coin {
        denom: "factory/cosmos2contract/owner/debt-suppliers".to_string(),
        amount: Uint128::new(500_000_000_000),
    }]);
    let msg = ExecuteMsg::WithdrawDebt {
        send_to: None,
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        withdraw_info,
        msg.clone(),
    )
    .unwrap();
    assert_eq!(res.messages.len(), 3); // Burn, send, rate assurance

    // Config check: total_debt_tokens adjusted
    let config: Config = from_binary(
        &query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap(),
    )
    .unwrap();
    assert_eq!(config.total_debt_tokens, Uint128::new(500_000));

    // Failure: Withdraw more than balance
    let msg = ExecuteMsg::WithdrawDebt {
        send_to: None,
    };
    let withdraw_info = mock_info(supplier, &[Coin {
        denom: "factory/cosmos2contract/owner/debt-suppliers".to_string(),
        amount: Uint128::new(10_000_000_000_000),
    }]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        withdraw_info,
        msg.clone(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
       "Custom Error val: \"Not enough debt tokens to send, maximum: 1000000\""
    );

    // Failure: Withdraw zero
    let msg = ExecuteMsg::WithdrawDebt {
        send_to: None,
    };
    let withdraw_info = mock_info(supplier, &[Coin {
        denom: "factory/cosmos2contract/owner/debt-suppliers".to_string(),
        amount: Uint128::new(0),
    }]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        withdraw_info,
        msg.clone(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "amount was zero, must be positive"
    );

    // Failure: Not whitelisted
    let msg = ExecuteMsg::WithdrawDebt {
        send_to: None,
    };
    let withdraw_info = mock_info("random_guy", &[Coin {
        denom: "factory/cosmos2contract/owner/debt-suppliers".to_string(),
        amount: Uint128::new(100_000_000_000),
    }]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        withdraw_info,
        msg.clone(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Custom Error val: \"Sender (\\\"random_guy\\\") not whitelisted to supply debt, so they can't withdraw either.\""
    );
    // Failure: Paused actions
    let pause_msg = ExecuteMsg::UpdateConfig {
        owner: None,
        osmosis_proxy_contract_addr: None,
        pause_actions: Some(true),
        manager_fee: None,
        whitelisted_debt_suppliers: None,
        debt_supply_cap: None,
    };
    execute(deps.as_mut(), env.clone(), admin_info, pause_msg).unwrap();

    let msg = ExecuteMsg::WithdrawDebt {
        send_to: None,
    };
    let withdraw_info = mock_info(supplier, &[Coin {
        denom: "factory/cosmos2contract/owner/debt-suppliers".to_string(),
        amount: Uint128::new(100_000_000_000),
    }]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        withdraw_info,
        msg,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Basket withdrawals & debt increases are frozen temporarily"
    );
}

}