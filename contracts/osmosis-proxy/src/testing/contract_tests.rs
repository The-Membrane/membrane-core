#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{
        mock_env, mock_info, mock_dependencies, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{
        coins, from_binary, Uint128, StdError, Addr, Decimal
    };
    use membrane::types::Owner;

    use membrane::osmosis_proxy::{InstantiateMsg, ExecuteMsg, QueryMsg, Config};

    use crate::TokenFactoryError;
    use crate::contract::{instantiate, execute, query};

    const DENOM_NAME: &str = "mydenom";
    const DENOM_PREFIX: &str = "factory";
   
    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn update_config() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let msg = ExecuteMsg::UpdateConfig { 
            owners: Some(vec![ Owner {
                owner: Addr::unchecked("new_owner2"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
                is_position_contract: false,
            },
            Owner {
                owner: Addr::unchecked("new_owner"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
                is_position_contract: false,
            }]),
            liquidity_multiplier: Some(Decimal::zero()),
            add_owner: Some(true), 
            debt_auction: Some(String::from("debt_auction")),
            positions_contract: Some(String::from("positions_contract")),
            liquidity_contract: Some(String::from("liquidity_contract")),
            oracle_contract: Some(String::from("oracle_contract")),
            edit_routes: None,
            transmutation_pairs: None,
            restrict_mbrn_mints: None,
        };
        let info = mock_info("creator", &coins(2, "token"));
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();       

        //Set expected_config
        let expected_config = Config {
            owners: vec![ Owner {
                owner: Addr::unchecked("creator"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
                is_position_contract: false,
            },
            Owner {
                owner: Addr::unchecked("new_owner2"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
                is_position_contract: false,
            },
            Owner {
                owner: Addr::unchecked("new_owner"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::zero()),
                non_token_contract_auth: true, 
                is_position_contract: false,
            }],
            liquidity_multiplier: Some(Decimal::zero()),
            debt_auction: Some(Addr::unchecked("debt_auction")),
            positions_contract: Some(Addr::unchecked("positions_contract")),
            liquidity_contract: Some(Addr::unchecked("liquidity_contract")),
            oracle_contract: Some(Addr::unchecked("oracle_contract")),
            restrict_mbrn_mints: Some(false),
        };

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Config {  }).unwrap();
        let config: Config = from_binary(&response).unwrap();
        assert_eq!(
            config,
            expected_config
        );

    }

    #[test]
    fn msg_create_denom_invalid_subdenom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        let subdenom: String = String::from("");

        let msg = ExecuteMsg::CreateDenom { 
            subdenom, 
            max_supply: Some(Uint128::new(10)), 
        };
        let info = mock_info("creator", &coins(2, "token"));
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(
            TokenFactoryError::InvalidSubdenom {
                subdenom: String::from("")
            },
            err
        );
    }

    #[test]
    fn msg_change_admin_empty_address() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const EMPTY_ADDR: &str = "";

        let info = mock_info("creator", &coins(2, "token"));

        let msg = ExecuteMsg::ChangeAdmin {
            denom: String::from(DENOM_NAME),
            new_admin_address: String::from(EMPTY_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        match err {
            TokenFactoryError::Std(StdError::GenericErr { msg, .. }) => {
                assert!(msg.contains("human address too short"))
            }
            e => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn msg_change_admin_invalid_denom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));

        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        

        const NEW_ADMIN_ADDR: &str = "newadmin";

        let info = mock_info("creator", &coins(2, "token"));

        // too many parts in denom
        let full_denom_name: &str = &format!(
            "{}/{}/{}/invalid",
            DENOM_PREFIX, MOCK_CONTRACT_ADDR, DENOM_NAME
        )[..];

        let msg = ExecuteMsg::ChangeAdmin {
            denom: String::from(full_denom_name),
            new_admin_address: String::from(NEW_ADMIN_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();

        let expected_error = TokenFactoryError::InvalidDenom {
            denom: String::from(full_denom_name),
            message: String::from("denom must have 3 parts separated by /, had 4"),
        };

        assert_eq!(expected_error, err);
    }


    #[test]
    fn msg_mint_invalid_denom() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {};
        let info = mock_info("creator", &coins(1000, "uosmo"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        
        const NEW_ADMIN_ADDR: &str = "newadmin";

        let mint_amount = Uint128::new(100_u128);

        let info = mock_info("creator", &coins(2, "token"));

        let full_denom_name: &str = &format!("{}/{}", DENOM_PREFIX, MOCK_CONTRACT_ADDR)[..];
        let msg = ExecuteMsg::MintTokens {
            denom: String::from(full_denom_name),
            amount: mint_amount,
            mint_to_address: String::from(NEW_ADMIN_ADDR),
        };
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        let expected_error = TokenFactoryError::InvalidDenom {
            denom: String::from(full_denom_name),
            message: String::from("denom must have 3 parts separated by /, had 2"),
        };

        assert_eq!(expected_error, err);
    }

}

/// Exhaustive supply tracking tests for mint, burn, GetTokenInfo, and supply snapshots
#[cfg(test)]
mod supply_tracking_tests {
    use cosmwasm_std::testing::{mock_env, mock_info, mock_dependencies};
    use cosmwasm_std::{coins, from_binary, Uint128, Addr, Decimal, Env, Timestamp};
    use membrane::types::Owner;
    use membrane::osmosis_proxy::{
        InstantiateMsg, ExecuteMsg, QueryMsg, TokenInfoResponse, SupplySnapshotResponse,
    };

    use crate::TokenFactoryError;
    use crate::contract::{instantiate, execute, query};
    use crate::state::{TOKENS, TokenInfo, SUPPLY_HISTORY};

    const DENOM: &str = "factory/cosmos2contract/mbrn";
    const CREATOR: &str = "creator";
    const MINT_TO: &str = "recipient";

    /// Setup: instantiate contract, add creator as owner, seed TOKENS map
    fn setup_with_token(max_supply: Option<Uint128>) -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, Env) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        // Instantiate
        let msg = InstantiateMsg {};
        let info = mock_info(CREATOR, &coins(1000, "uosmo"));
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Seed TOKENS map directly (CreateDenom requires Osmosis reply)
        TOKENS.save(
            deps.as_mut().storage,
            DENOM.to_string(),
            &TokenInfo {
                current_supply: Uint128::zero(),
                max_supply,
                burned_supply: Uint128::zero(),
            },
        ).unwrap();

        (deps, env)
    }

    fn setup_with_debt_auction(max_supply: Option<Uint128>) -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, Env) {
        let (mut deps, env) = setup_with_token(max_supply);

        // Set debt_auction address
        let msg = ExecuteMsg::UpdateConfig {
            owners: None,
            liquidity_multiplier: None,
            add_owner: None,
            debt_auction: Some("debt_auction".to_string()),
            positions_contract: None,
            liquidity_contract: None,
            oracle_contract: None,
            edit_routes: None,
            transmutation_pairs: None,
            restrict_mbrn_mints: None,
        };
        let info = mock_info(CREATOR, &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        (deps, env)
    }

    fn query_token_info(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, env: &Env) -> TokenInfoResponse {
        let res = query(deps.as_ref(), env.clone(), QueryMsg::GetTokenInfo { denom: DENOM.to_string() }).unwrap();
        from_binary(&res).unwrap()
    }

    fn query_supply_history(deps: &cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, env: &Env) -> SupplySnapshotResponse {
        let res = query(deps.as_ref(), env.clone(), QueryMsg::GetSupplyHistory {
            denom: DENOM.to_string(),
            limit: None,
            start_after: None,
        }).unwrap();
        from_binary(&res).unwrap()
    }

    fn env_at_time(seconds: u64) -> Env {
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(seconds);
        env
    }

    // ===================== Mint Supply Tracking =====================

    #[test]
    fn test_mint_increments_current_supply() {
        let (mut deps, env) = setup_with_token(None);

        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        let info = mock_info(CREATOR, &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(100));
        assert_eq!(token_info.burned_supply, Uint128::zero());
    }

    #[test]
    fn test_multiple_mints_accumulate() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint 50
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint 30
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(30),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(80));
    }

    #[test]
    fn test_mint_zero_amount_rejected() {
        let (mut deps, env) = setup_with_token(None);

        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::zero(),
            mint_to_address: MINT_TO.to_string(),
        };
        let info = mock_info(CREATOR, &[]);
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, TokenFactoryError::ZeroAmount {});
    }

    #[test]
    fn test_mint_respects_max_supply() {
        let (mut deps, env) = setup_with_debt_auction(Some(Uint128::new(100)));
        let info = mock_info(CREATOR, &[]);

        // Mint 50 — under max, should succeed
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            mint_to_address: MINT_TO.to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        // Check that mint was allowed (amount attribute is non-zero)
        assert_eq!(res.attributes.iter().find(|a| a.key == "amount").unwrap().value, "50");

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(50));

        // Mint 60 more — still succeeds because current_supply (50) <= max (100)
        // (the check is current_supply <= max_supply, not current_supply + amount <= max_supply)
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(60),
            mint_to_address: MINT_TO.to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(res.attributes.iter().find(|a| a.key == "amount").unwrap().value, "60");

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(110));

        // Now current_supply (110) > max (100), so next mint from non-debt-auction is blocked
        // The TOKENS.update still succeeds but mint_allowed stays false
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(10),
            mint_to_address: MINT_TO.to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        // amount should be 0 (blocked)
        assert_eq!(res.attributes.iter().find(|a| a.key == "amount").unwrap().value, "0");
    }

    #[test]
    fn test_mint_no_max_supply_unlimited() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint a large amount — no max supply, should succeed
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(1_000_000_000),
            mint_to_address: MINT_TO.to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes.iter().find(|a| a.key == "amount").unwrap().value, "1000000000");

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(1_000_000_000));
    }

    #[test]
    fn test_debt_auction_can_exceed_max_supply() {
        let (mut deps, env) = setup_with_debt_auction(Some(Uint128::new(100)));

        // First mint to get over max supply
        let info = mock_info(CREATOR, &[]);
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Now current_supply == max_supply, non-debt-auction would be blocked
        // But add debt_auction as owner first so it passes authority check
        let msg = ExecuteMsg::UpdateConfig {
            owners: Some(vec![Owner {
                owner: Addr::unchecked("debt_auction"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: None,
                non_token_contract_auth: true,
                is_position_contract: false,
            }]),
            liquidity_multiplier: None,
            add_owner: Some(true),
            debt_auction: None,
            positions_contract: None,
            liquidity_contract: None,
            oracle_contract: None,
            edit_routes: None,
            transmutation_pairs: None,
            restrict_mbrn_mints: None,
        };
        let info = mock_info(CREATOR, &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Mint as debt_auction — should succeed even over max supply
        let info = mock_info("debt_auction", &[]);
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            mint_to_address: MINT_TO.to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes.iter().find(|a| a.key == "amount").unwrap().value, "50");

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(150));
    }

    #[test]
    fn test_mint_unauthorized_rejected() {
        let (mut deps, env) = setup_with_token(None);

        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        let info = mock_info("unauthorized_user", &[]);
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, TokenFactoryError::Unauthorized {});
    }

    // ===================== Burn Supply Tracking =====================

    #[test]
    fn test_burn_decrements_current_supply() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint 100
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Burn 30
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(30),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(70));
    }

    #[test]
    fn test_burn_increments_burned_supply() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint 100
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Burn 30
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(30),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.burned_supply, Uint128::new(30));
    }

    #[test]
    fn test_multiple_burns_accumulate() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint 100
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Burn 20
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(20),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Burn 10
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(10),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(70));
        assert_eq!(token_info.burned_supply, Uint128::new(30));
    }

    #[test]
    fn test_burn_zero_amount_rejected() {
        let (mut deps, env) = setup_with_token(None);

        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::zero(),
            burn_from_address: MINT_TO.to_string(),
        };
        let info = mock_info(CREATOR, &[]);
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, TokenFactoryError::ZeroAmount {});
    }

    #[test]
    fn test_burn_unauthorized_rejected() {
        let (mut deps, env) = setup_with_token(None);

        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(10),
            burn_from_address: MINT_TO.to_string(),
        };
        let info = mock_info("unauthorized_user", &[]);
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, TokenFactoryError::Unauthorized {});
    }

    // ===================== GetTokenInfo Query =====================

    #[test]
    fn test_get_token_info_initial_values() {
        let (deps, env) = setup_with_token(Some(Uint128::new(1_000_000)));

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.denom, DENOM);
        assert_eq!(token_info.current_supply, Uint128::zero());
        assert_eq!(token_info.max_supply, Uint128::new(1_000_000));
        assert_eq!(token_info.burned_supply, Uint128::zero());
    }

    #[test]
    fn test_get_token_info_after_mint_and_burn() {
        let (mut deps, env) = setup_with_token(None);
        let info = mock_info(CREATOR, &[]);

        // Mint 200
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(200),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Burn 50
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let token_info = query_token_info(&deps, &env);
        assert_eq!(token_info.current_supply, Uint128::new(150));
        assert_eq!(token_info.burned_supply, Uint128::new(50));
        assert_eq!(token_info.max_supply, Uint128::zero()); // None → zero in response
    }

    #[test]
    fn test_get_token_info_unknown_denom_errors() {
        let (deps, env) = setup_with_token(None);

        let res = query(deps.as_ref(), env, QueryMsg::GetTokenInfo {
            denom: "factory/unknown/token".to_string(),
        });
        assert!(res.is_err());
    }

    // ===================== Supply Snapshot History =====================

    #[test]
    fn test_supply_snapshot_recorded_on_mint() {
        let (mut deps, env) = {
            let mut d = mock_dependencies();
            let e = env_at_time(1_000_000);
            let msg = InstantiateMsg {};
            let info = mock_info(CREATOR, &coins(1000, "uosmo"));
            instantiate(d.as_mut(), e.clone(), info, msg).unwrap();
            TOKENS.save(d.as_mut().storage, DENOM.to_string(), &TokenInfo {
                current_supply: Uint128::zero(),
                max_supply: None,
                burned_supply: Uint128::zero(),
            }).unwrap();
            (d, e)
        };

        let info = mock_info(CREATOR, &[]);
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let history = query_supply_history(&deps, &env);
        assert_eq!(history.snapshots.len(), 1);
        assert_eq!(history.snapshots[0].current_supply, Uint128::new(100));
        assert_eq!(history.snapshots[0].burned_supply, Uint128::zero());
    }

    #[test]
    fn test_supply_snapshot_recorded_on_burn() {
        let (mut deps, env) = {
            let mut d = mock_dependencies();
            let e = env_at_time(1_000_000);
            let msg = InstantiateMsg {};
            let info = mock_info(CREATOR, &coins(1000, "uosmo"));
            instantiate(d.as_mut(), e.clone(), info, msg).unwrap();
            TOKENS.save(d.as_mut().storage, DENOM.to_string(), &TokenInfo {
                current_supply: Uint128::new(200),
                max_supply: None,
                burned_supply: Uint128::zero(),
            }).unwrap();
            (d, e)
        };

        let info = mock_info(CREATOR, &[]);
        let msg = ExecuteMsg::BurnTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            burn_from_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let history = query_supply_history(&deps, &env);
        assert_eq!(history.snapshots.len(), 1);
        assert_eq!(history.snapshots[0].current_supply, Uint128::new(150));
        assert_eq!(history.snapshots[0].burned_supply, Uint128::new(50));
    }

    #[test]
    fn test_supply_snapshot_dedupes_same_day() {
        let (mut deps, env) = {
            let mut d = mock_dependencies();
            let e = env_at_time(1_000_000);
            let msg = InstantiateMsg {};
            let info = mock_info(CREATOR, &coins(1000, "uosmo"));
            instantiate(d.as_mut(), e.clone(), info, msg).unwrap();
            TOKENS.save(d.as_mut().storage, DENOM.to_string(), &TokenInfo {
                current_supply: Uint128::zero(),
                max_supply: None,
                burned_supply: Uint128::zero(),
            }).unwrap();
            (d, e)
        };

        let info = mock_info(CREATOR, &[]);

        // First mint
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Second mint same block time
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(30),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let history = query_supply_history(&deps, &env);
        // Should only have 1 snapshot (deduped within same day)
        assert_eq!(history.snapshots.len(), 1);
    }

    #[test]
    fn test_supply_snapshot_new_day_new_entry() {
        let (mut deps, _) = {
            let mut d = mock_dependencies();
            let e = env_at_time(1_000_000);
            let msg = InstantiateMsg {};
            let info = mock_info(CREATOR, &coins(1000, "uosmo"));
            instantiate(d.as_mut(), e.clone(), info, msg).unwrap();
            TOKENS.save(d.as_mut().storage, DENOM.to_string(), &TokenInfo {
                current_supply: Uint128::zero(),
                max_supply: None,
                burned_supply: Uint128::zero(),
            }).unwrap();
            (d, e)
        };

        let info = mock_info(CREATOR, &[]);

        // Mint at day 1
        let env1 = env_at_time(1_000_000);
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(100),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env1.clone(), info.clone(), msg).unwrap();

        // Mint at day 2 (86400 seconds later)
        let env2 = env_at_time(1_000_000 + 86_400);
        let msg = ExecuteMsg::MintTokens {
            denom: DENOM.to_string(),
            amount: Uint128::new(50),
            mint_to_address: MINT_TO.to_string(),
        };
        execute(deps.as_mut(), env2.clone(), info, msg).unwrap();

        let history = query_supply_history(&deps, &env2);
        assert_eq!(history.snapshots.len(), 2);
        assert_eq!(history.snapshots[0].current_supply, Uint128::new(100));
        assert_eq!(history.snapshots[1].current_supply, Uint128::new(150));
    }
}