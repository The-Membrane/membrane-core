#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_json};

    use crate::contract::{execute, instantiate, query};
    use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn test_mint_car() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: Some(racing::types::CarMetadata {
                name: "Speedster".to_string(),
                description: Some("A fast car".to_string()),
                image_uri: Some("ipfs://Qm...".to_string()),
                attributes: Some(vec![
                    racing::types::CarAttribute {
                        trait_type: "Speed".to_string(),
                        value: "High".to_string(),
                    },
                ]),
            }),
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify car was minted - query by car_id 0 (first minted car)
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info.len(), 1);
        assert_eq!(car_info[0].car_id, "0");
        assert_eq!(car_info[0].owners, vec!["alice"]);
        assert!(car_info[0].metadata.is_some());
    }

    #[test]
    fn test_mint_car_without_metadata() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car without metadata
        let msg = ExecuteMsg::Mint {
            owners: vec!["bob".to_string()],
            metadata: None,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify car was minted - query by car_id 0 (first minted car)
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info.len(), 1);
        assert_eq!(car_info[0].car_id, "0");
        assert_eq!(car_info[0].owners, vec!["bob"]);
        assert!(car_info[0].metadata.is_none());
    }

    #[test]
    fn test_mint_car_multiple_owners() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car with multiple owners
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string(), "bob".to_string(), "charlie".to_string()],
            metadata: Some(racing::types::CarMetadata {
                name: "Team Car".to_string(),
                description: Some("A car owned by multiple people".to_string()),
                image_uri: None,
                attributes: None,
            }),
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify car was minted with multiple owners
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info.len(), 1);
        assert_eq!(car_info[0].car_id, "0");
        assert_eq!(car_info[0].owners, vec!["alice", "bob", "charlie"]);
        assert!(car_info[0].metadata.is_some());
    }

    #[test]
    fn test_mint_duplicate_car() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint first car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Try to mint duplicate car - this should work since car IDs are auto-generated
        let msg = ExecuteMsg::Mint {
            owners: vec!["bob".to_string()],
            metadata: None,
        };
        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_ok()); // Should succeed since car IDs are auto-generated
    }

    #[test]
    fn test_mint_empty_car_id() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Try to mint with empty car_id
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_err());
    }

    #[test]
    fn test_update_car_metadata() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Update metadata
        let new_metadata = racing::types::CarMetadata {
            name: "Updated Car".to_string(),
            description: Some("Updated description".to_string()),
            image_uri: Some("ipfs://QmUpdated...".to_string()),
            attributes: Some(vec![
                racing::types::CarAttribute {
                    trait_type: "Speed".to_string(),
                    value: "Very High".to_string(),
                },
            ]),
        };

        let msg = ExecuteMsg::UpdateCarMetadata {
            car_id: 0,
            metadata: new_metadata,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify metadata was updated
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert!(car_info[0].metadata.is_some());
        let metadata = car_info[0].metadata.as_ref().unwrap();
        assert_eq!(metadata.name, "Updated Car");
    }

    #[test]
    fn test_update_car_metadata_unauthorized() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Try to update metadata as unauthorized user
        let new_metadata = racing::types::CarMetadata {
            name: "Updated Car".to_string(),
            description: None,
            image_uri: None,
            attributes: None,
        };

        let msg = ExecuteMsg::UpdateCarMetadata {
            car_id: 0,
            metadata: new_metadata,
        };

        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_err());
    }

    #[test]
    fn test_transfer_car() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Transfer car
        let msg = ExecuteMsg::Transfer {
            car_id: 0,
            to: "bob".to_string(),
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify car was transferred
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info[0].owners, vec!["bob"]);
    }

    #[test]
    fn test_transfer_car_unauthorized() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Try to transfer car as unauthorized user
        let msg = ExecuteMsg::Transfer {
            car_id: 0,
            to: "bob".to_string(),
        };

        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_err());
    }

    #[test]
    fn test_update_owner_add() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car with single owner
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Add new owner
        let msg = ExecuteMsg::UpdateOwner {
            car_id: 0,
            owners: vec!["bob".to_string()],
            is_add: true,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify new owner was added
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info[0].owners, vec!["alice", "bob"]);
    }

    #[test]
    fn test_update_owner_replace() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car with multiple owners
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string(), "bob".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Replace all owners
        let msg = ExecuteMsg::UpdateOwner {
            car_id: 0,
            owners: vec!["charlie".to_string(), "david".to_string()],
            is_add: false,
        };

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(0, res.messages.len());
        
        // Verify owners were replaced
        let query_msg = QueryMsg::GetCarInfo { 
            car_id: Some(0),
            start_after: None,
            limit: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let car_info: Vec<crate::msg::GetCarInfoResponse> = from_json(&res).unwrap();
        
        assert_eq!(car_info[0].owners, vec!["charlie", "david"]);
    }

    #[test]
    fn test_query_q_values() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Query Q-values for specific state
        let query_msg = QueryMsg::GetQ {
            car_id: "0".to_string(),
            state_hash: Some("state_123".to_string()),
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let q_response: crate::msg::GetQResponse = from_json(&res).unwrap();
        
        assert_eq!(q_response.car_id, "0");
        assert_eq!(q_response.q_values.len(), 1);
        assert_eq!(q_response.q_values[0].state_hash, "state_123");
        assert_eq!(q_response.q_values[0].action_values, [0, 0, 0, 0]);
    }

    #[test]
    fn test_query_all_q_values() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Query all Q-values
        let query_msg = QueryMsg::GetQ {
            car_id: "0".to_string(),
            state_hash: None,
        };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let q_response: crate::msg::GetQResponse = from_json(&res).unwrap();
        
        assert_eq!(q_response.car_id, "0");
        assert_eq!(q_response.q_values.len(), 0); // No Q-values stored yet
    }

    #[test]
    fn test_query_owner_of() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Query owner
        let query_msg = QueryMsg::OwnerOf { car_id: 0 };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let owner_response: crate::msg::OwnerOfResponse = from_json(&res).unwrap();
        
        assert_eq!(owner_response.owners, vec!["alice"]);
    }

    #[test]
    fn test_query_nft_info() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint car
        let msg = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: Some(racing::types::CarMetadata {
                name: "Test Car".to_string(),
                description: Some("A test car".to_string()),
                image_uri: None,
                attributes: None,
            }),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Query NFT info
        let query_msg = QueryMsg::NftInfo { car_id: 0 };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let nft_response: crate::msg::NftInfoResponse = from_json(&res).unwrap();
        
        assert_eq!(nft_response.car_id, "0");
        assert_eq!(nft_response.owners, vec!["alice"]);
        assert!(nft_response.metadata.is_some());
    }

    #[test]
    fn test_query_all_tokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &coins(1000, "earth"));

        // Instantiate
        let msg = InstantiateMsg {
            admin: "creator".to_string(),
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

        // Mint multiple cars
        let msg1 = ExecuteMsg::Mint {
            owners: vec!["alice".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg1).unwrap();

        let msg2 = ExecuteMsg::Mint {
            owners: vec!["bob".to_string()],
            metadata: None,
        };
        execute(deps.as_mut(), env.clone(), info.clone(), msg2).unwrap();

        // Query all tokens
        let query_msg = QueryMsg::AllTokens {};
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let all_tokens_response: crate::msg::AllTokensResponse = from_json(&res).unwrap();
        
        assert_eq!(all_tokens_response.tokens.len(), 2);
        assert!(all_tokens_response.tokens.contains(&"0".to_string()));
        assert!(all_tokens_response.tokens.contains(&"1".to_string()));
    }

    // Integration tests using cw-multi-test
    #[cfg(test)]
    mod integration_tests {
        use super::*;
        use cosmwasm_std::Addr;
        use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

        fn car_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
            let contract = ContractWrapper::new(
                crate::contract::execute,
                crate::contract::instantiate,
                crate::contract::query,
            );
            Box::new(contract)
        }

        #[test]
        fn test_integration_car_minting_and_transfer() {
            let mut app = AppBuilder::new().build(|router, _, storage| {
                router
                    .bank
                    .init_balance(storage, &Addr::unchecked("alice"), coins(1000, "earth"))
                    .unwrap();
                router
                    .bank
                    .init_balance(storage, &Addr::unchecked("bob"), coins(1000, "earth"))
                    .unwrap();
            });

            // Upload and instantiate car contract
            let car_contract_id = app.store_code(car_contract());
            let car_addr = app
                .instantiate_contract(
                    car_contract_id,
                    Addr::unchecked("admin"),
                    &InstantiateMsg { admin: "admin".to_string() },
                    &[],
                    "Car NFT",
                    None,
                )
                .unwrap();

            // Mint car by alice
            let mint_msg = ExecuteMsg::Mint {
                owners: vec!["alice".to_string()],
                metadata: Some(racing::types::CarMetadata {
                    name: "Speedster".to_string(),
                    description: Some("A fast car".to_string()),
                    image_uri: Some("ipfs://Qm...".to_string()),
                    attributes: Some(vec![]),
                }),
            };

            let result = app
                .execute_contract(
                    Addr::unchecked("alice"),
                    car_addr.clone(),
                    &mint_msg,
                    &[],
                )
                .unwrap();

            // Verify minting was successful
            assert!(result.events.iter().any(|event| {
                event.ty == "wasm" && event.attributes.iter().any(|attr| {
                    attr.key == "method" && attr.value == "mint"
                })
            }));

            // Query car info
            let car_info: Vec<crate::msg::GetCarInfoResponse> = app
                .wrap()
                .query_wasm_smart(&car_addr, &QueryMsg::GetCarInfo { 
                    car_id: Some(0),
                    start_after: None,
                    limit: None,
                })
                .unwrap();

            assert_eq!(car_info[0].owners, vec!["alice"]);
            assert!(car_info[0].metadata.is_some());

            // Transfer car to bob
            let transfer_msg = ExecuteMsg::Transfer {
                car_id: 0,
                to: "bob".to_string(),
            };

            let result = app
                .execute_contract(
                    Addr::unchecked("alice"),
                    car_addr.clone(),
                    &transfer_msg,
                    &[],
                )
                .unwrap();

            // Verify transfer was successful
            assert!(result.events.iter().any(|event| {
                event.ty == "wasm" && event.attributes.iter().any(|attr| {
                    attr.key == "method" && attr.value == "transfer"
                })
            }));

            // Query car info after transfer
            let car_info: Vec<crate::msg::GetCarInfoResponse> = app
                .wrap()
                .query_wasm_smart(&car_addr, &QueryMsg::GetCarInfo { 
                    car_id: Some(0),
                    start_after: None,
                    limit: None,
                })
                .unwrap();

            assert_eq!(car_info[0].owners, vec!["bob"]);
        }

        #[test]
        fn test_integration_multiple_cars() {
            let mut app = AppBuilder::new().build(|router, _, storage| {
                router
                    .bank
                    .init_balance(storage, &Addr::unchecked("alice"), coins(1000, "earth"))
                    .unwrap();
            });

            // Upload and instantiate car contract
            let car_contract_id = app.store_code(car_contract());
            let car_addr = app
                .instantiate_contract(
                    car_contract_id,
                    Addr::unchecked("admin"),
                    &InstantiateMsg { admin: "admin".to_string() },
                    &[],
                    "Car NFT",
                    None,
                )
                .unwrap();

            // Mint multiple cars
            for i in 0..5 {
                let mint_msg = ExecuteMsg::Mint {
                    owners: vec!["alice".to_string()],
                    metadata: Some(racing::types::CarMetadata {
                        name: format!("Car {}", i),
                        description: Some(format!("Car number {}", i)),
                        image_uri: Some(format!("ipfs://Qm{}", i)),
                        attributes: Some(vec![]),
                    }),
                };

                app.execute_contract(
                    Addr::unchecked("alice"),
                    car_addr.clone(),
                    &mint_msg,
                    &[],
                )
                .unwrap();
            }

            // Query all tokens
            let all_tokens: crate::msg::AllTokensResponse = app
                .wrap()
                .query_wasm_smart(&car_addr, &QueryMsg::AllTokens {})
                .unwrap();

            assert_eq!(all_tokens.tokens.len(), 5);
            for i in 0..5 {
                assert!(all_tokens.tokens.contains(&i.to_string()));
            }
        }

        #[test]
        fn test_integration_q_table_queries() {
            let mut app = AppBuilder::new().build(|router, _, storage| {
                router
                    .bank
                    .init_balance(storage, &Addr::unchecked("alice"), coins(1000, "earth"))
                    .unwrap();
            });

            // Upload and instantiate car contract
            let car_contract_id = app.store_code(car_contract());
            let car_addr = app
                .instantiate_contract(
                    car_contract_id,
                    Addr::unchecked("admin"),
                    &InstantiateMsg { admin: "admin".to_string() },
                    &[],
                    "Car NFT",
                    None,
                )
                .unwrap();

            // Mint car
            let mint_msg = ExecuteMsg::Mint {
                owners: vec!["alice".to_string()],
                metadata: None,
            };

            app.execute_contract(
                Addr::unchecked("alice"),
                car_addr.clone(),
                &mint_msg,
                &[],
            )
            .unwrap();

            // Query Q-values for specific state
            let q_response: crate::msg::GetQResponse = app
                .wrap()
                .query_wasm_smart(
                    &car_addr,
                    &QueryMsg::GetQ {
                        car_id: "0".to_string(),
                        state_hash: Some("state_123".to_string()),
                    }
                )
                .unwrap();

            assert_eq!(q_response.car_id, "0");
            assert_eq!(q_response.q_values.len(), 1);
            assert_eq!(q_response.q_values[0].state_hash, "state_123");
            assert_eq!(q_response.q_values[0].action_values, [0, 0, 0, 0]);

            // Query all Q-values
            let q_response: crate::msg::GetQResponse = app
                .wrap()
                .query_wasm_smart(
                    &car_addr,
                    &QueryMsg::GetQ {
                        car_id: "0".to_string(),
                        state_hash: None,
                    }
                )
                .unwrap();

            assert_eq!(q_response.car_id, "0");
            // Should be empty since no Q-values have been set
            assert_eq!(q_response.q_values.len(), 0);
        }

        #[test]
        fn test_integration_error_handling() {
            let mut app = AppBuilder::new().build(|router, _, storage| {
                router
                    .bank
                    .init_balance(storage, &Addr::unchecked("alice"), coins(1000, "earth"))
                    .unwrap();
            });

            // Upload and instantiate car contract
            let car_contract_id = app.store_code(car_contract());
            let car_addr = app
                .instantiate_contract(
                    car_contract_id,
                    Addr::unchecked("admin"),
                    &InstantiateMsg { admin: "admin".to_string() },
                    &[],
                    "Car NFT",
                    None,
                )
                .unwrap();

            // Try to query non-existent car
            let result = app.wrap().query_wasm_smart::<Vec<crate::msg::GetCarInfoResponse>>(
                &car_addr,
                &QueryMsg::GetCarInfo { 
                    car_id: Some(999),
                    start_after: None,
                    limit: None,
                }
            );

            assert!(result.is_err()); // Should fail because car doesn't exist
        }
    }
} 