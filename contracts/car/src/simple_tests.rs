#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate};
    use crate::state::{CONFIG, USED_TRAIT_COMBOS};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, Addr, Order};
    use membrane::car::{Config, ExecuteMsg, InstantiateMsg};
    use membrane::types::CarMetadata;

    fn test_name_key(name_trimmed: &str) -> u128 {
        let mut h: u128 = 0x9E37_79B9_7F4A_7C15_6C8E_9CF5_9D1B_BCD7u128;
        for b in name_trimmed.as_bytes() {
            h ^= (*b as u128).wrapping_mul(0x100_0000_01B3);
            h = h.rotate_left(13).wrapping_mul(0xC2B2_AE3D_27D4_EB4Fu128);
        }
        h
    }

    const CREATOR: &str = "creator";
    const CONTRACT_NAME: &str = "Test Car NFT";
    const CONTRACT_SYMBOL: &str = "TCAR";

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(CREATOR, &[]);

        let msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: Some(coins(100, "uosmo")),
        };

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert!(res.messages.len() >= 0);

        // Verify config was saved
        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, Addr::unchecked(CREATOR));
        assert_eq!(config.payment_options, coins(100, "uosmo"));
    }

    #[test]
    fn test_instantiate_without_payment_options() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info(CREATOR, &[]);

        let msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: None,
        };

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert!(res.messages.len() >= 0);

        // Verify config was saved with empty payment options
        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, Addr::unchecked(CREATOR));
        assert_eq!(config.payment_options, vec![]);
    }

    #[test]
    fn test_mint_car() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let creator_info = mock_info(CREATOR, &[]);

        // First instantiate
        let instantiate_msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: None,
        };
        instantiate(deps.as_mut(), env.clone(), creator_info.clone(), instantiate_msg).unwrap();

        // Now test minting a car
        let owner = "car_owner";
        let metadata = CarMetadata {
            name: "Test Car".to_string(),
            image_data: Some("data:image/svg+xml;base64,...".to_string()),
            attributes: Some(vec![
                membrane::types::CarAttribute {
                    trait_type: "Speed".to_string(),
                    value: "High".to_string(),
                },
            ]),
            car_id: None, // Will be auto-populated by the contract
        };

        let mint_msg = ExecuteMsg::CreateCar {
            owner: Some(owner.to_string()),
            token_uri: Some("ipfs://QmTest...".to_string()),
            extension: Some(metadata.clone()),
        };

        let res = execute(deps.as_mut(), env, creator_info, mint_msg).unwrap();
        // Should have a self-call message for the cw721 mint
        assert!(res.messages.len() > 0);
    }

    #[test]
    fn test_update_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let creator_info = mock_info(CREATOR, &[]);

        // First instantiate
        let instantiate_msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: None,
        };
        instantiate(deps.as_mut(), env.clone(), creator_info.clone(), instantiate_msg).unwrap();

        // Test updating config
        let new_payment_options = coins(200, "uosmo");
        let update_msg = ExecuteMsg::UpdateConfig {
            payment_options: Some(new_payment_options.clone()),
            new_owner: None,
            race_engine_contract: None,
        };

        let res = execute(deps.as_mut(), env, creator_info, update_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // Verify config was updated
        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.payment_options, new_payment_options);
    }

    #[test]
    fn test_generate_unique_cars() {
        let unique_combos = 256;
        let mut deps = mock_dependencies();
        let env = mock_env();
        let creator_info = mock_info(CREATOR, &[]);

        // Instantiate without payment options
        let instantiate_msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: None,
        };
        instantiate(deps.as_mut(), env.clone(), creator_info.clone(), instantiate_msg).unwrap();

        // Mint 256 cars; uniqueness is enforced by the contract and combos are stored
        for i in 0..unique_combos {
            let owner = format!("owner_{}", i);
            let mint_msg = ExecuteMsg::CreateCar {
                owner: Some(owner),
                token_uri: None,
                extension: Some(CarMetadata { name: format!("Car {}", i), image_data: None, attributes: None, car_id: None }),
            };
            let _ = execute(deps.as_mut(), env.clone(), creator_info.clone(), mint_msg).unwrap();
        }

        // Count used combos in storage
        let mut count: usize = 0;
        for item in USED_TRAIT_COMBOS.range(&deps.storage, None, None, Order::Ascending) {
            let _ = item.unwrap();
            count += 1;
        }
        assert_eq!(count, unique_combos);

        // Print all stored combos for inspection
        let mut entries: Vec<(u64, bool)> = Vec::new();
        for item in USED_TRAIT_COMBOS.range(&deps.storage, None, None, Order::Ascending) {
            let (k, v) = item.unwrap();
            entries.push((k, v));
        }
        println!("used_trait_combos: {:?}", entries);
    }

    #[test]
    fn test_free_pending_creation_and_finalize() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let creator_info = mock_info(CREATOR, &[]);

        // Instantiate with both free and paid options
        let instantiate_msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: Some(vec![
                cosmwasm_std::coin(10, "uosmo"),             // paid
                cosmwasm_std::coin(1, "free"),               // 1 minute free
            ]),
        };
        instantiate(deps.as_mut(), env.clone(), creator_info.clone(), instantiate_msg).unwrap();

        // Create pending free car: send no funds -> should create PENDING_FREE_CARS
        let owner = "owner1";
        let mint_msg = ExecuteMsg::CreateCar {
            owner: Some(owner.to_string()),
            token_uri: None,
            extension: Some(CarMetadata { name: "FreeCar".to_string(), image_data: None, attributes: None, car_id: None }),
        };
        let res = execute(deps.as_mut(), env.clone(), creator_info.clone(), mint_msg).unwrap();
        assert_eq!(0, res.messages.len());
        // Verify pending stored at car_id 1 (since 0 is reserved)
        let pending = crate::state::PENDING_FREE_CARS.load(&deps.storage, 1u128).unwrap();
        assert_eq!(pending.reserved_for, Addr::unchecked(owner));
        assert!(pending.expires_at_nanos > env.block.time.nanos());

        // Finalize by paying a non-free denom from ANY sender (finalize loosened)
        let finalize_msg = ExecuteMsg::PayToFinalize { token_id: "1".to_string() };
        let payer_info = mock_info("payer", &vec![cosmwasm_std::coin(10, "uosmo")]);
        let res = execute(deps.as_mut(), env.clone(), payer_info, finalize_msg).unwrap();
        // Should include cw721 self-mint message
        assert!(res.messages.len() > 0);
        // Pending should be removed
        assert!(crate::state::PENDING_FREE_CARS.load(&deps.storage, 1u128).is_err());
    }

    #[test]
    fn test_free_pending_expire_and_purge_msg() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let creator_info = mock_info(CREATOR, &[]);

        // Instantiate with only free option
        let instantiate_msg = InstantiateMsg {
            name: CONTRACT_NAME.to_string(),
            symbol: CONTRACT_SYMBOL.to_string(),
            payment_options: Some(vec![ cosmwasm_std::coin(1, "free") ]), // 1 minute
        };
        instantiate(deps.as_mut(), env.clone(), creator_info.clone(), instantiate_msg).unwrap();

        // Set race engine address in config
        let _ = execute(deps.as_mut(), env.clone(), creator_info.clone(), ExecuteMsg::UpdateConfig {
            payment_options: None,
            new_owner: None,
            race_engine_contract: Some("race_engine".to_string()),
        }).unwrap();

        // Create pending free car id 1
        let mint_msg = ExecuteMsg::CreateCar {
            owner: Some("user".to_string()),
            token_uri: None,
            extension: Some(CarMetadata { name: "FreeCar2".to_string(), image_data: None, attributes: None, car_id: None }),
        };
        let _ = execute(deps.as_mut(), env.clone(), creator_info.clone(), mint_msg).unwrap();
        // Capture trait code and name key before expire
        let pending = crate::state::PENDING_FREE_CARS.load(&deps.storage, 1u128).unwrap();
        let car_info = crate::state::CAR_INFO.load(&deps.storage, 1u128).unwrap();
        let name_key_before = test_name_key(car_info.metadata.as_ref().unwrap().name.trim());
        assert!(crate::state::NAME_REGISTRY.has(&deps.storage, name_key_before));
        assert!(crate::state::USED_TRAIT_COMBOS.has(&deps.storage, pending.trait_code));

        // Advance time beyond expiration (add 2 minutes)
        let new_nanos = pending.expires_at_nanos + 120_000_000_000; // 120s
        env.block.time = cosmwasm_std::Timestamp::from_nanos(new_nanos);

        // Expire
        let res = execute(deps.as_mut(), env.clone(), creator_info.clone(), ExecuteMsg::ExpireCar { token_id: "1".to_string() }).unwrap();
        // Should include purge message to race_engine
        assert!(res.messages.len() == 1);
        if let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { contract_addr, .. }) = &res.messages[0].msg {
            assert_eq!(contract_addr, "race_engine");
        } else { panic!("expected purge execute msg"); }

        // Pending removed
        assert!(crate::state::PENDING_FREE_CARS.load(&deps.storage, 1u128).is_err());
        // Name freed
        assert!(!crate::state::NAME_REGISTRY.has(&deps.storage, name_key_before));
        // Trait combo freed
        assert!(!crate::state::USED_TRAIT_COMBOS.has(&deps.storage, pending.trait_code));
    }
} 