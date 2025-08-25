use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Uint128};

use crate::contract::{execute, instantiate, query};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::types::{TileProperties, Track};

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
fn test_add_track() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track with minimum 3x3 size
    let layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Test Track".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify track was added
    let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let track_response: Track = from_json(&res).unwrap();
    
    assert_eq!(track_response.id, 0u128);
    assert_eq!(track_response.name, "Test Track");
    assert_eq!(track_response.width, 3);
    assert_eq!(track_response.height, 3);
    assert_eq!(track_response.layout.len(), 3);
}

#[test]
fn test_add_multiple_tracks() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add multiple tracks with minimum 3x3 size
    for i in 1..=3 {
        let layout = vec![
            vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
            vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ];

        let msg = ExecuteMsg::AddTrack {
            name: format!("Track {}", i),
            width: 3,
            height: 3,
            layout,
        };

        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    }
    
    // Verify all tracks were added
    for i in 0..3u128 {
        let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(i) };
        let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
        let track_response: Track = from_json(&res).unwrap();
        
        assert_eq!(track_response.id, i);
        assert!(track_response.name.starts_with("Track "));
    }
}

#[test]
fn test_add_track_with_complex_layout() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track with complex layout (4x4)
    let layout = vec![
        vec![TileProperties::start(), TileProperties::boost(2), TileProperties { speed_modifier: 0, ..Default::default() }, TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::sticky(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Complex Track".to_string(),
        width: 4,
        height: 4,
        layout,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify complex track was added
    let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let track_response: Track = from_json(&res).unwrap();
    
    assert_eq!(track_response.id, 0u128);
    assert_eq!(track_response.name, "Complex Track");
    assert_eq!(track_response.width, 4);
    assert_eq!(track_response.height, 4);
    assert_eq!(track_response.layout.len(), 4);
    assert_eq!(track_response.layout[0].len(), 4);
}

#[test]
fn test_list_tracks() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add multiple tracks with minimum 3x3 size
    for i in 1..=3 {
        let layout = vec![
            vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
            vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ];

        let msg = ExecuteMsg::AddTrack {
            name: format!("Track {}", i),
            width: 3,
            height: 3,
            layout,
        };

        execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    }
    
    // List all tracks
    let query_msg = QueryMsg::ListTracks { start_after: None, limit: None };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let list_response: crate::msg::ListTracksResponse = from_json(&res).unwrap();
    
    assert_eq!(list_response.tracks.len(), 3);
    for i in 0..3u128 {
        assert!(list_response.tracks.iter().any(|track| track.id == i));
    }
}

#[test]
fn test_add_track_with_different_tile_types() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track with all tile types (3x3)
    let layout = vec![
        vec![TileProperties::start(), TileProperties::boost(2), TileProperties { speed_modifier: 0, ..Default::default() }],
        vec![TileProperties::wall(), TileProperties::sticky(), TileProperties::finish()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "All Tiles Track".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify track was added with all tile types
    let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let track_response: Track = from_json(&res).unwrap();
    
    assert_eq!(track_response.layout[0][0].properties.is_start, true);
    assert_eq!(track_response.layout[0][1].properties.speed_modifier, 2);
    assert!(track_response.layout[0][2].properties.speed_modifier < 1);
    assert_eq!(track_response.layout[1][0].properties.blocks_movement, true);
    assert_eq!(track_response.layout[1][1].properties.skip_next_turn, true);
    assert_eq!(track_response.layout[1][2].properties.is_finish, true);
}

#[test]
fn test_add_track_with_large_dimensions() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track with large dimensions
    let width = 10;
    let height = 8;
    let mut layout = vec![vec![TileProperties::normal(); width]; height];
    
    // Add finish line at the top
    for x in 0..width {
        layout[0][x] = TileProperties::finish();
    }
    // add at least one start tile
    layout[height-1][0] = TileProperties::start();
    
    // Add some obstacles
    layout[5][5] = TileProperties::wall();
    layout[3][3] = TileProperties::sticky();
    layout[7][7] = TileProperties::boost(2);

    let msg = ExecuteMsg::AddTrack {
        name: "Large Track".to_string(),
        width: width as u8,
        height: height as u8,
        layout,
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify large track was added
    let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let track_response: Track = from_json(&res).unwrap();
    
    assert_eq!(track_response.width, width as u8);
    assert_eq!(track_response.height, height as u8);
    assert_eq!(track_response.layout.len(), height);
    assert_eq!(track_response.layout[0].len(), width);
}

// Integration tests using cw-multi-test
#[cfg(test)]
mod integration_tests {
    use super::*;
    use cosmwasm_std::Addr;
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

    fn track_manager_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
        let contract = ContractWrapper::new(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

    #[test]
    fn test_integration_track_creation_and_query() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate track manager contract
        let track_manager_contract_id = app.store_code(track_manager_contract());
        let track_manager_addr = app
            .instantiate_contract(
                track_manager_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg { admin: "admin".to_string() },
                &[],
                "Track Manager",
                None,
            )
            .unwrap();

        // Add track
        let layout = vec![
            vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
            vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "Test Track".to_string(),
            width: 3,
            height: 3,
            layout,
        };

        let result = app
            .execute_contract(
                Addr::unchecked("admin"),
                track_manager_addr.clone(),
                &add_track_msg,
                &[],
            )
            .unwrap();

        // Verify track creation was successful
        assert!(result.events.iter().any(|event| {
            event.ty == "wasm" && event.attributes.iter().any(|attr| {
                attr.key == "method" && attr.value == "add_track"
            })
        }));

        // Query track
        let track: Track = app
            .wrap()
            .query_wasm_smart(&track_manager_addr, &QueryMsg::GetTrack { track_id: Uint128::from(0u128) })
            .unwrap();

        assert_eq!(track.id, 0u128);
        assert_eq!(track.name, "Test Track");
        assert_eq!(track.width, 3);
        assert_eq!(track.height, 3);
        assert_eq!(track.layout.len(), 3);
    }

    #[test]
    fn test_integration_multiple_tracks() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate track manager contract
        let track_manager_contract_id = app.store_code(track_manager_contract());
        let track_manager_addr = app
            .instantiate_contract(
                track_manager_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg { admin: "admin".to_string() },
                &[],
                "Track Manager",
                None,
            )
            .unwrap();

        // Add multiple tracks
        for i in 1..=5 {
            let layout = vec![
                vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
                vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ];

            let add_track_msg = ExecuteMsg::AddTrack {
                name: format!("Track {}", i),
                width: 3,
                height: 3,
                layout,
            };

            app.execute_contract(
                Addr::unchecked("admin"),
                track_manager_addr.clone(),
                &add_track_msg,
                &[],
            )
            .unwrap();
        }

        // List all tracks
        let tracks: crate::msg::ListTracksResponse = app
            .wrap()
            .query_wasm_smart(&track_manager_addr, &QueryMsg::ListTracks { start_after: None, limit: None })
            .unwrap();

        assert_eq!(tracks.tracks.len(), 5);
        for i in 0..5u128 {
            assert!(tracks.tracks.iter().any(|track| track.id == i));
        }
    }

    #[test]
    fn test_integration_complex_track_layout() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate track manager contract
        let track_manager_contract_id = app.store_code(track_manager_contract());
        let track_manager_addr = app
            .instantiate_contract(
                track_manager_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg { admin: "admin".to_string() },
                &[],
                "Track Manager",
                None,
            )
            .unwrap();

        // Add complex track
        let layout = vec![
            vec![
                TileProperties::start(),
                TileProperties::boost(2),
                TileProperties { speed_modifier: 0, ..Default::default() },
                TileProperties::finish(),
            ],
            vec![
                TileProperties::wall(),
                TileProperties::normal(),
                TileProperties::sticky(),
                TileProperties::normal(),
            ],
            vec![
                TileProperties::normal(),
                TileProperties::wall(),
                TileProperties::normal(),
                TileProperties::boost(2),
            ],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "Complex Track".to_string(),
            width: 4,
            height: 3,
            layout,
        };

        app.execute_contract(
            Addr::unchecked("admin"),
            track_manager_addr.clone(),
            &add_track_msg,
            &[],
        )
        .unwrap();

        // Query complex track
        let track: Track = app
            .wrap()
            .query_wasm_smart(&track_manager_addr, &QueryMsg::GetTrack { track_id: Uint128::from(0u128) })
            .unwrap();

        assert_eq!(track.id, 0u128);
        assert_eq!(track.name, "Complex Track");
        assert_eq!(track.width, 4);
        assert_eq!(track.height, 3);
        assert_eq!(track.layout.len(), 3);
        assert_eq!(track.layout[0].len(), 4);

        // Verify specific tile types
        assert!(track.layout[0][0].properties.is_start);
        assert_eq!(track.layout[0][1].properties.speed_modifier, 2);
        assert!(track.layout[0][2].properties.speed_modifier < 1);
        assert!(track.layout[0][3].properties.is_finish);
        assert!(track.layout[1][0].properties.blocks_movement);
        assert!(track.layout[1][2].properties.skip_next_turn);
    }

    #[test]
    fn test_integration_track_validation() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate track manager contract
        let track_manager_contract_id = app.store_code(track_manager_contract());
        let track_manager_addr = app
            .instantiate_contract(
                track_manager_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg { admin: "admin".to_string() },
                &[],
                "Track Manager",
                None,
            )
            .unwrap();

        // Try to add track with mismatched dimensions
        let layout = vec![
            vec![TileProperties::start(), TileProperties::finish()],
            vec![TileProperties::wall(), TileProperties::normal()],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "Invalid Track".to_string(),
            width: 3, // Mismatched with layout width of 2
            height: 2,
            layout,
        };

        let result = app.execute_contract(
            Addr::unchecked("admin"),
            track_manager_addr.clone(),
            &add_track_msg,
            &[],
        );

        assert!(result.is_err()); // Should fail due to dimension mismatch
    }

    #[test]
    fn test_integration_error_handling() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate track manager contract
        let track_manager_contract_id = app.store_code(track_manager_contract());
        let track_manager_addr = app
            .instantiate_contract(
                track_manager_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg { admin: "admin".to_string() },
                &[],
                "Track Manager",
                None,
            )
            .unwrap();

        // Try to query non-existent track
        let result = app.wrap().query_wasm_smart::<Track>(
            &track_manager_addr,
            &QueryMsg::GetTrack { track_id: Uint128::from(9999u128) }
        );

        assert!(result.is_err()); // Should fail because track doesn't exist

        // Try to add track with empty name
        let layout = vec![
            vec![TileProperties::start(), TileProperties::finish()],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "".to_string(), // Empty name
            width: 2,
            height: 1,
            layout,
        };

        let result = app.execute_contract(
            Addr::unchecked("admin"),
            track_manager_addr.clone(),
            &add_track_msg,
            &[],
        );

        assert!(result.is_err()); // Should fail due to empty name
    }
} 