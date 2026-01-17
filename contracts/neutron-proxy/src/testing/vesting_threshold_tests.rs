#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate};
    use crate::state::{CONFIG, TOKENS, TRANSMUTE_SUPPLY_THRESHOLDS};

    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, Addr, Decimal, Uint128, CosmosMsg, WasmMsg};
    use std::str::FromStr;

    use membrane::neutron_proxy::{ExecuteMsg, InstantiateMsg, TransmutationPair, TransmutationPairEntry};
    use membrane::types::{Owner, VestingPeriod};

    const SECONDS_IN_DAY: u64 = 86400;

    fn mock_instantiate_msg() -> InstantiateMsg {
        InstantiateMsg {
            owners: vec![Owner {
                owner: Addr::unchecked("owner"),
                total_minted: Uint128::zero(),
                stability_pool_ratio: Some(Decimal::from_str("0.5").unwrap()),
                non_token_contract_auth: false,
                is_position_contract: false,
            }],
            liquidity_multiplier: Decimal::from_str("1.0").unwrap(),
            debt_auction: Some(Addr::unchecked("debt_auction")),
            positions_contract: Some(Addr::unchecked("positions")),
            liquidity_contract: Some(Addr::unchecked("liquidity")),
            oracle_contract: Some(Addr::unchecked("oracle")),
            transmutation_pairs: vec![],
        }
    }

    #[test]
    fn test_transmutation_redirects_to_vesting_after_threshold() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Set up transmutation pair
        let transmutation_pairs = vec![TransmutationPairEntry {
            transmutation_pair: TransmutationPair {
                token_in: "old_mbrn".to_string(),
                token_to_mint: "factory/contract/new_mbrn".to_string(),
                mint_ratio: Decimal::from_str("1.0").unwrap(),
            },
            remove: false,
        }];

        // Set vesting config
        let vesting_period = VestingPeriod {
            cliff: 180, // 180 days
            linear: 180, // 180 days
        };

        let update_config_msg = ExecuteMsg::UpdateConfig {
            owners: None,
            debt_auction: None,
            transmutation_pairs: Some(transmutation_pairs),
            transmuter_contract: None,
            vaults: None,
            astroport_factory: None,
            astroport_router: None,
            enable_dynamic_routing: None,
            transmute_supply_thresholds: None,
            vesting_contract: Some("vesting_contract".to_string()),
            vesting_period: Some(vesting_period.clone()),
        };

        execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

        // Create the token
        let create_denom_msg = ExecuteMsg::CreateDenom {
            subdenom: "new_mbrn".to_string(),
            max_supply: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), create_denom_msg).unwrap();

        // Set threshold at 10,000
        let set_threshold_msg = ExecuteMsg::SetTransmuteSupplyThreshold {
            denom: "factory/contract/new_mbrn".to_string(),
            threshold: Uint128::new(10_000),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), set_threshold_msg).unwrap();

        // Manually set current supply to exceed threshold
        TOKENS
            .update(deps.as_mut().storage, "factory/contract/new_mbrn".to_string(), |token| {
                let mut t = token.unwrap();
                t.current_supply = Uint128::new(15_000); // Above threshold
                Ok::<_, cosmwasm_std::StdError>(t)
            })
            .unwrap();

        // Try to transmute - should redirect to vesting
        let transmute_info = mock_info("user", &coins(1000, "old_mbrn"));
        let transmute_msg = ExecuteMsg::TransmuteTokens {};
        let res = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

        // Verify response
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "vesting_transmutation"));
        assert!(res.attributes.iter().any(|a| a.key == "user" && a.value == "user"));
        assert!(res.attributes.iter().any(|a| a.key == "amount_to_vest" && a.value == "1000"));

        // Verify it's sending a message to vesting contract
        assert_eq!(res.messages.len(), 1);

        // Verify the message structure
        match &res.messages[0].msg {
            CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, msg, funds }) => {
                assert_eq!(contract_addr, "vesting_contract");
                assert_eq!(funds.len(), 1);
                assert_eq!(funds[0].denom, "old_mbrn");
                assert_eq!(funds[0].amount, Uint128::new(1000));

                // Verify the message is AddVestedTransmutation
                let decoded: membrane::vesting::ExecuteMsg = cosmwasm_std::from_json(msg).unwrap();
                match decoded {
                    membrane::vesting::ExecuteMsg::AddVestedTransmutation {
                        recipient,
                        amount_to_mint,
                        vesting_period: vp,
                    } => {
                        assert_eq!(recipient, "user");
                        assert_eq!(amount_to_mint, Uint128::new(1000));
                        assert_eq!(vp.cliff, vesting_period.cliff);
                        assert_eq!(vp.linear, vesting_period.linear);
                    }
                    _ => panic!("Expected AddVestedTransmutation message"),
                }
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }

    #[test]
    fn test_transmutation_blocked_before_threshold() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Set up transmutation pair
        let transmutation_pairs = vec![TransmutationPairEntry {
            transmutation_pair: TransmutationPair {
                token_in: "old_mbrn".to_string(),
                token_to_mint: "factory/contract/new_mbrn".to_string(),
                mint_ratio: Decimal::from_str("1.0").unwrap(),
            },
            remove: false,
        }];

        let update_config_msg = ExecuteMsg::UpdateConfig {
            owners: None,
            debt_auction: None,
            transmutation_pairs: Some(transmutation_pairs),
            transmuter_contract: None,
            vaults: None,
            astroport_factory: None,
            astroport_router: None,
            enable_dynamic_routing: None,
            transmute_supply_thresholds: None,
            vesting_contract: None,
            vesting_period: None,
        };

        execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

        // Create the token and set supply threshold at 10,000
        let create_denom_msg = ExecuteMsg::CreateDenom {
            subdenom: "new_mbrn".to_string(),
            max_supply: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), create_denom_msg).unwrap();

        // Set threshold
        let set_threshold_msg = ExecuteMsg::SetTransmuteSupplyThreshold {
            denom: "factory/contract/new_mbrn".to_string(),
            threshold: Uint128::new(10_000),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), set_threshold_msg).unwrap();

        // Try to transmute before threshold - should fail
        let transmute_info = mock_info("user", &coins(1000, "old_mbrn"));
        let transmute_msg = ExecuteMsg::TransmuteTokens {};
        let err = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap_err();

        assert!(err.to_string().contains("Transmuting not yet enabled"));
        assert!(err.to_string().contains("Current supply: 0"));
        assert!(err.to_string().contains("required threshold: 10000"));
    }

    #[test]
    fn test_vesting_contract_not_configured_error() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Set up transmutation pair WITHOUT vesting config
        let transmutation_pairs = vec![TransmutationPairEntry {
            transmutation_pair: TransmutationPair {
                token_in: "old_mbrn".to_string(),
                token_to_mint: "factory/contract/new_mbrn".to_string(),
                mint_ratio: Decimal::from_str("1.0").unwrap(),
            },
            remove: false,
        }];

        let update_config_msg = ExecuteMsg::UpdateConfig {
            owners: None,
            debt_auction: None,
            transmutation_pairs: Some(transmutation_pairs),
            transmuter_contract: None,
            vaults: None,
            astroport_factory: None,
            astroport_router: None,
            enable_dynamic_routing: None,
            transmute_supply_thresholds: None,
            vesting_contract: None, // NOT CONFIGURED
            vesting_period: None,
        };

        execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

        // Create token and set threshold
        let create_denom_msg = ExecuteMsg::CreateDenom {
            subdenom: "new_mbrn".to_string(),
            max_supply: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), create_denom_msg).unwrap();

        let set_threshold_msg = ExecuteMsg::SetTransmuteSupplyThreshold {
            denom: "factory/contract/new_mbrn".to_string(),
            threshold: Uint128::new(10_000),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), set_threshold_msg).unwrap();

        // Set supply above threshold
        TOKENS
            .update(deps.as_mut().storage, "factory/contract/new_mbrn".to_string(), |token| {
                let mut t = token.unwrap();
                t.current_supply = Uint128::new(15_000);
                Ok::<_, cosmwasm_std::StdError>(t)
            })
            .unwrap();

        // Try to transmute - should fail with vesting not configured error
        let transmute_info = mock_info("user", &coins(1000, "old_mbrn"));
        let transmute_msg = ExecuteMsg::TransmuteTokens {};
        let err = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap_err();

        assert!(err.to_string().contains("Vesting contract not configured"));
    }
}
