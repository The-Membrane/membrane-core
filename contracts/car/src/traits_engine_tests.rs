#[cfg(test)]
mod traits_engine_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_json, Coin};

    use crate::contract::{execute, instantiate};
    use crate::msg::{ExecuteMsg, InstantiateMsg};
    use cw721_base::{ExecuteMsg as Cw721ExecuteMsg, MintMsg};

    use racing::types::{CarAttribute, CarMetadata};

    fn setup(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) {
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let msg = InstantiateMsg {
            name: "Membrane Cars".to_string(),
            symbol: "CAR".to_string(),
            payment_options: None, // free mint for tests
        };
        instantiate(deps.as_mut(), env, info, msg).unwrap();
    }

    #[test]
    fn mint_appends_traits_and_rarity_when_no_initial_metadata() {
        let mut deps = mock_dependencies();
        setup(&mut deps);

        let env = mock_env();
        let info = mock_info("minter", &[]);

        // Execute mint with no initial metadata; contract will create default and append traits
        let exec = ExecuteMsg::MintCar {
            owner: "alice".to_string(),
            token_uri: Some("ipfs://car.png".to_string()),
            extension: None,
        };

        let resp = execute(deps.as_mut(), env, info, exec).unwrap();
        assert_eq!(resp.messages.len(), 1);

        // Inspect the cw721 mint message
        let msg = &resp.messages[0].msg;
        let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = msg else { panic!("expected Wasm Execute") };
        let cw: Cw721ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty> = from_json(msg).unwrap();
        let Cw721ExecuteMsg::Mint(MintMsg { extension, .. }) = cw else { panic!("expected cw721 Mint") };
        let meta = extension.expect("metadata must be present");

        // Expect attributes present and include the single rarity trait
        let attrs = meta.attributes.expect("attributes must be present");
        assert!(!attrs.is_empty());
        assert!(attrs.iter().any(|a| a.trait_type == "rarity"));

        // Expect a decent number of traits appended
        // We append 23 traits (22 features + 1 rarity)
        assert!(attrs.len() >= 23);
    }

    #[test]
    fn mint_appends_traits_and_keeps_existing_attributes() {
        let mut deps = mock_dependencies();
        setup(&mut deps);

        let env = mock_env();
        let info = mock_info("minter", &[]);

        // Provide initial metadata with one attribute
        let initial = CarMetadata {
            name: "Custom".to_string(),
            description: Some("desc".to_string()),
            image_uri: None,
            attributes: Some(vec![CarAttribute { trait_type: "Speed".to_string(), value: "High".to_string() }]),
            car_id: None,
        };

        let exec = ExecuteMsg::MintCar {
            owner: "bob".to_string(),
            token_uri: None,
            extension: Some(initial),
        };

        let resp = execute(deps.as_mut(), env, info, exec).unwrap();
        assert_eq!(resp.messages.len(), 1);

        // Inspect the cw721 mint message and verify attributes appended
        let msg = &resp.messages[0].msg;
        let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = msg else { panic!("expected Wasm Execute") };
        let cw: Cw721ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty> = from_json(msg).unwrap();
        let Cw721ExecuteMsg::Mint(MintMsg { extension, .. }) = cw else { panic!("expected cw721 Mint") };
        let meta = extension.expect("metadata must be present");

        let attrs = meta.attributes.expect("attributes must be present");
        // 1 existing + 23 appended
        assert!(attrs.len() >= 24);
        assert!(attrs.iter().any(|a| a.trait_type == "Speed" && a.value == "High"));
        assert!(attrs.iter().any(|a| a.trait_type == "rarity"));
        // car_id should be set by contract
        assert!(meta.car_id.is_some());
    }

    #[test]
    fn update_custom_decal_sets_raw_svg() {
        let mut deps = mock_dependencies();
        setup(&mut deps);

        // 1) Mint via our high-level message to get the cw721 Mint message
        let env = mock_env();
        let info = mock_info("minter", &[]);
        let exec = ExecuteMsg::MintCar {
            owner: "alice".to_string(),
            token_uri: None,
            extension: None,
        };
        let resp = execute(deps.as_mut(), env.clone(), info, exec).unwrap();
        assert_eq!(resp.messages.len(), 1);

        // 2) Execute the embedded cw721 Mint as the contract (minter)
        let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = &resp.messages[0].msg else { panic!("expected Wasm Execute") };
        let cw: Cw721ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty> = from_json(msg).unwrap();
        let token_id_owned = match &cw { Cw721ExecuteMsg::Mint(MintMsg { token_id, .. }) => token_id.clone(), _ => panic!("expected cw721 Mint") };

        // Call our Base execute to run the cw721 mint with sender = contract address
        let info_minter = mock_info(env.contract.address.as_str(), &[]);
        let _ = execute(deps.as_mut(), env.clone(), info_minter, ExecuteMsg::Base(cw)).unwrap();

        // 3) Owner updates the custom decal SVG
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>".to_string();
        let info_owner = mock_info("alice", &[]);
        let _ = execute(
            deps.as_mut(),
            env.clone(),
            info_owner,
            ExecuteMsg::UpdateCustomDecal { token_id: token_id_owned.clone(), svg: svg.clone() },
        )
        .unwrap();

        // 4) Query NftInfo and verify decal attribute equals the raw SVG
        use cw721::NftInfoResponse;
        let q = crate::msg::QueryMsg::Base(cw721_base::QueryMsg::NftInfo { token_id: token_id_owned.clone() });
        let bin = crate::contract::query(deps.as_ref(), env, q).unwrap();
        let nft_info: NftInfoResponse<Option<CarMetadata>> = from_json(&bin).unwrap();
        let attrs = nft_info.extension.unwrap().attributes.unwrap_or_default();
        let decal_attr = attrs.iter().find(|a| a.trait_type == "decal").expect("decal attribute missing");
        assert_eq!(decal_attr.value, svg);
    }

    #[test]
    fn update_custom_decal_fails_for_preset() {
        let mut deps = mock_dependencies();
        setup(&mut deps);

        // Mint a token as before
        let env = mock_env();
        let info = mock_info("minter", &[]);
        let exec = ExecuteMsg::MintCar {
            owner: "alice".to_string(),
            token_uri: None,
            extension: None,
        };
        let resp = execute(deps.as_mut(), env.clone(), info, exec).unwrap();
        let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = &resp.messages[0].msg else { panic!("expected Wasm Execute") };
        let cw: Cw721ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty> = from_json(msg).unwrap();
        let token_id_owned = match &cw { Cw721ExecuteMsg::Mint(MintMsg { token_id, .. }) => token_id.clone(), _ => panic!("expected cw721 Mint") };
        let info_minter = mock_info(env.contract.address.as_str(), &[]);
        let _ = execute(deps.as_mut(), env.clone(), info_minter, ExecuteMsg::Base(cw)).unwrap();

        // Overwrite metadata decal attribute to a preset marker to simulate a non-custom decal
        // Query, mutate, and save via direct state access similar to update handler
        use cw721_base::Cw721Contract;
        type CarNFT<'a> = Cw721Contract<'a, Option<CarMetadata>, cosmwasm_std::Empty, cosmwasm_std::Empty, cosmwasm_std::Empty>;
        let contract: CarNFT = Cw721Contract::default();
        let mut token = contract.tokens.load(deps.as_ref().storage, &token_id_owned).unwrap();
        let mut ext = token.extension.take().unwrap_or(CarMetadata { name: String::new(), description: None, image_uri: None, attributes: None, car_id: Some(token_id_owned.clone()) });
        let mut found = false;
        if let Some(attrs) = &mut ext.attributes {
            for a in attrs.iter_mut() { if a.trait_type == "decal" { a.value = "Preset::FlamesA".to_string(); found = true; break; } }
            if !found { attrs.push(racing::types::CarAttribute { trait_type: "decal".to_string(), value: "Preset::FlamesA".to_string() }); }
        } else {
            ext.attributes = Some(vec![racing::types::CarAttribute { trait_type: "decal".to_string(), value: "Preset::FlamesA".to_string() }]);
        }
        token.extension = Some(ext);
        contract.tokens.save(deps.as_mut().storage, &token_id_owned, &token).unwrap();

        // Attempt to update should fail with NotCustomDecal
        let info_owner = mock_info("alice", &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info_owner,
            ExecuteMsg::UpdateCustomDecal { token_id: token_id_owned.clone(), svg: "<svg/>".to_string() },
        )
        .unwrap_err();
        assert!(format!("{}", err).contains("Decal is not custom"));
    }
} 