#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate};
    use crate::state::{CONFIG, get_car_info};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, Addr};
    use membrane::car::{Config, ExecuteMsg, InstantiateMsg};
    use membrane::types::CarMetadata;

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
        assert_eq!(0, res.messages.len());

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
        assert_eq!(0, res.messages.len());

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
            car_id: None, // Will be auto-populated
        };

        let mint_msg = ExecuteMsg::MintCar {
            owner: owner.to_string(),
            token_uri: Some("ipfs://QmTest...".to_string()),
            extension: Some(metadata.clone()),
        };

        let res = execute(deps.as_mut(), env, creator_info, mint_msg).unwrap();
        
        // Should have messages for the cw721 mint
        assert!(res.messages.len() > 0);
        
        // Verify car info was saved
        let car_info = get_car_info(&deps.storage, 0).unwrap();
        assert_eq!(car_info.owners.len(), 1);
        assert_eq!(car_info.owners[0], Addr::unchecked(owner));
        assert_eq!(car_info.metadata, Some(metadata));
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
        };

        let res = execute(deps.as_mut(), env, creator_info, update_msg).unwrap();
        assert_eq!(0, res.messages.len());

        // Verify config was updated
        let config: Config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.payment_options, new_payment_options);
    }
} 