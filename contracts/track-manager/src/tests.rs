use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Uint128};

use crate::contract::{execute, instantiate, query};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::types::{TileProperties, Track, CompressedTrack, CompressedTrackLayout};

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

    // Add multiple tracks with different layouts
    for i in 1..=3 {
        let layout = match i {
            1 => vec![
                vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
                vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
            2 => vec![
                vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
                vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
            _ => vec![
                vec![TileProperties::finish(), TileProperties::normal(), TileProperties::start()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::wall()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
        };

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

    // Add multiple tracks with different layouts
    for i in 1..=3 {
        let layout = match i {
            1 => vec![
                vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
                vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
            2 => vec![
                vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
                vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
            _ => vec![
                vec![TileProperties::finish(), TileProperties::normal(), TileProperties::start()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::wall()],
                vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
            ],
        };

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

        // Add multiple tracks with different layouts
        for i in 1..=5 {
            let layout = match i {
                1 => vec![
                    vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
                    vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                ],
                2 => vec![
                    vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
                    vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                ],
                3 => vec![
                    vec![TileProperties::finish(), TileProperties::normal(), TileProperties::start()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::wall()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                ],
                4 => vec![
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                    vec![TileProperties::start(), TileProperties::wall(), TileProperties::finish()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                ],
                _ => vec![
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                    vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
                    vec![TileProperties::start(), TileProperties::wall(), TileProperties::finish()],
                ],
            };

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

        // Try to add track with no finish tile
        let layout = vec![
            vec![TileProperties::start(), TileProperties::normal()],
            vec![TileProperties::wall(), TileProperties::normal()],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "Invalid Track".to_string(),
            width: 2,
            height: 2,
            layout,
        };

        let result = app.execute_contract(
            Addr::unchecked("admin"),
            track_manager_addr.clone(),
            &add_track_msg,
            &[],
        );

        assert!(result.is_err()); // Should fail due to no finish tile
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

        // Try to add track with no start tile
        let layout = vec![
            vec![TileProperties::normal(), TileProperties::finish()],
        ];

        let add_track_msg = ExecuteMsg::AddTrack {
            name: "No Start Track".to_string(),
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

        assert!(result.is_err()); // Should fail due to no start tile
    }
}

#[test]
fn test_tile_compression() {
    // Test that empty tiles are properly compressed
    let empty_tile = TileProperties::default();
    assert!(empty_tile.is_empty());
    
    let compressed = empty_tile.compress();
    assert!(compressed.is_none());
    
    let decompressed = TileProperties::decompress(compressed);
    assert!(decompressed.is_empty());
    
    // Test that non-empty tiles are not compressed
    let wall_tile = TileProperties::wall();
    assert!(!wall_tile.is_empty());
    
    let compressed_wall = wall_tile.compress();
    assert!(compressed_wall.is_some());
    
    let decompressed_wall = TileProperties::decompress(compressed_wall);
    assert!(decompressed_wall.blocks_movement);
}

#[test]
fn test_compressed_track_layout() {
    // Create a layout with mostly empty tiles
    let layout = vec![
        vec![TileProperties::start(), TileProperties::default(), TileProperties::finish()],
        vec![TileProperties::default(), TileProperties::wall(), TileProperties::default()],
        vec![TileProperties::default(), TileProperties::default(), TileProperties::default()],
    ];
    
    // Test compression
    let compressed_layout = CompressedTrackLayout::from_full_layout(&layout);
    assert_eq!(compressed_layout.width, 3);
    assert_eq!(compressed_layout.height, 3);
    
    // Check that empty tiles are compressed (None)
    assert!(compressed_layout.compressed_layout[0][1].is_none()); // Empty tile
    assert!(compressed_layout.compressed_layout[1][0].is_none()); // Empty tile
    assert!(compressed_layout.compressed_layout[2][0].is_none()); // Empty tile
    
    // Check that non-empty tiles are preserved
    assert!(compressed_layout.compressed_layout[0][0].is_some()); // Start tile
    assert!(compressed_layout.compressed_layout[0][2].is_some()); // Finish tile
    assert!(compressed_layout.compressed_layout[1][1].is_some()); // Wall tile
    
    // Test decompression
    let decompressed_layout = compressed_layout.to_full_layout();
    assert_eq!(decompressed_layout.len(), 3);
    assert_eq!(decompressed_layout[0].len(), 3);
    
    // Verify that decompressed layout matches original
    assert!(decompressed_layout[0][0].is_start);
    assert!(decompressed_layout[0][1].is_empty());
    assert!(decompressed_layout[0][2].is_finish);
    assert!(decompressed_layout[1][0].is_empty());
    assert!(decompressed_layout[1][1].blocks_movement);
    assert!(decompressed_layout[1][2].is_empty());
}

#[test]
fn test_compressed_track_storage_and_retrieval() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Create a track with many empty tiles
    let layout = vec![
        vec![TileProperties::start(), TileProperties::default(), TileProperties::default(), TileProperties::finish()],
        vec![TileProperties::default(), TileProperties::wall(), TileProperties::default(), TileProperties::default()],
        vec![TileProperties::default(), TileProperties::default(), TileProperties::default(), TileProperties::default()],
        vec![TileProperties::default(), TileProperties::default(), TileProperties::default(), TileProperties::default()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Compressed Test Track".to_string(),
        width: 4,
        height: 4,
        layout,
    };

    // Add track (should compress automatically)
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Query the track back (first track gets ID 0)
    let query_msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let track: Track = from_json(&res).unwrap();
    
    // Verify the track was properly expanded from compressed storage
    assert_eq!(track.name, "Compressed Test Track");
    assert_eq!(track.width, 4);
    assert_eq!(track.height, 4);
    assert_eq!(track.layout.len(), 4);
    assert_eq!(track.layout[0].len(), 4);
    
    // Verify specific tiles
    assert!(track.layout[0][0].properties.is_start);
    assert!(track.layout[0][1].properties.is_empty());
    assert!(track.layout[0][3].properties.is_finish);
    assert!(track.layout[1][1].properties.blocks_movement);
}

#[test]
fn test_edit_track_name() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track
    let layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Original Track".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Edit track name
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(0u128),
        name: Some("Updated Track Name".to_string()),
        delete: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());
    assert_eq!(res.attributes[1].value, "0"); // track_id
    assert_eq!(res.attributes[2].value, "name_update"); // action
    assert_eq!(res.attributes[3].value, "Updated Track Name"); // new_name

    // Verify the track name was updated
    let msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), env, msg).unwrap();
    let track: Track = from_json(res).unwrap();
    assert_eq!(track.name, "Updated Track Name");
}

#[test]
fn test_edit_track_delete() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track
    let layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Track To Delete".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Delete track
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(0u128),
        name: None,
        delete: Some(true),
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());
    assert_eq!(res.attributes[1].value, "0"); // track_id
    assert_eq!(res.attributes[2].value, "delete"); // action
    assert_eq!(res.attributes[3].value, "Track To Delete"); // track_name

    // Verify the track was deleted
    let msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), env, msg);
    assert!(res.is_err()); // Track should not exist
}

#[test]
fn test_edit_track_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let creator_info = mock_info("creator", &coins(1000, "earth"));
    let other_info = mock_info("other_user", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), creator_info.clone(), msg).unwrap();

    // Add track as creator
    let layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Creator's Track".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env.clone(), creator_info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Try to edit track as different user
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(0u128),
        name: Some("Hacked Track Name".to_string()),
        delete: None,
    };

    let res = execute(deps.as_mut(), env.clone(), other_info, msg);
    assert!(res.is_err()); // Should fail due to unauthorized access
}

#[test]
fn test_edit_track_invalid_operation() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Try to edit track with no operation specified
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(0u128),
        name: None,
        delete: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(res.is_err()); // Should fail due to invalid operation
}

#[test]
fn test_duplicate_track_name() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add first track
    let layout1 = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "My Track".to_string(),
        width: 3,
        height: 3,
        layout: layout1,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Try to add second track with same name but different layout
    let layout2 = vec![
        vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "My Track".to_string(), // Same name
        width: 3,
        height: 3,
        layout: layout2,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_err()); // Should fail due to duplicate name
}

#[test]
fn test_edit_track_duplicate_name() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add first track
    let layout1 = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Track One".to_string(),
        width: 3,
        height: 3,
        layout: layout1,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Add second track with different name
    let layout2 = vec![
        vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Track Two".to_string(),
        width: 3,
        height: 3,
        layout: layout2,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Try to edit second track to have same name as first track
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(1u128),
        name: Some("Track One".to_string()), // Same name as first track
        delete: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_err()); // Should fail due to duplicate name
}

#[test]
fn test_edit_track_name_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add track
    let layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Original Name".to_string(),
        width: 3,
        height: 3,
        layout,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Edit track name to a new unique name
    let msg = ExecuteMsg::EditTrack {
        track_id: Uint128::from(0u128),
        name: Some("New Unique Name".to_string()),
        delete: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(0, res.messages.len());
    assert_eq!(res.attributes[2].value, "name_update");
    assert_eq!(res.attributes[3].value, "New Unique Name");

    // Verify the track name was updated
    let msg = QueryMsg::GetTrack { track_id: Uint128::from(0u128) };
    let res = query(deps.as_ref(), env, msg).unwrap();
    let track: Track = from_json(res).unwrap();
    assert_eq!(track.name, "New Unique Name");
}

#[test]
fn test_migrate_populates_name_hashes() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Add multiple tracks with different names
    let layouts = vec![
        vec![
            vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
            vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ],
        vec![
            vec![TileProperties::normal(), TileProperties::start(), TileProperties::normal()],
            vec![TileProperties::normal(), TileProperties::wall(), TileProperties::finish()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ],
        vec![
            vec![TileProperties::finish(), TileProperties::normal(), TileProperties::start()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::wall()],
            vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
        ],
    ];

    let names = vec!["Track One", "Track Two", "Track Three"];

    // Add tracks directly to storage without name hashes (simulating old tracks)
    for (i, (layout, name)) in layouts.iter().zip(names.iter()).enumerate() {
        let track_id = Uint128::from(i as u128);
        
        // Create track manually without name hash validation
        let track = Track {
            creator: info.sender.to_string(),
            id: track_id.into(),
            name: name.to_string(),
            width: 3,
            height: 3,
            layout: vec![vec![membrane::types::TrackTile {
                properties: TileProperties::default(),
                progress_towards_finish: 0,
                min_steps_to_finish_from_start: None,
                x: 0,
                y: 0,
            }; 3]; 3],
            fastest_tick_time: 0,
            starting_tiles: vec![],
        };
        
        crate::state::set_track(deps.as_mut().storage, &track_id.u128(), track).unwrap();
    }

    // Now run migrate to populate name hashes
    let migrate_msg = membrane::track_manager::MigrateMsg {};
    let res = crate::contract::migrate(deps.as_mut(), env.clone(), migrate_msg).unwrap();
    
    // Check that migration was successful
    assert_eq!(res.attributes[0].value, "migrate");
    assert_eq!(res.attributes[1].value, "3"); // migrated_tracks
    assert_eq!(res.attributes[2].value, "0"); // error_count

    // Verify that duplicate name detection now works
    let duplicate_layout = vec![
        vec![TileProperties::start(), TileProperties::normal(), TileProperties::finish()],
        vec![TileProperties::wall(), TileProperties::normal(), TileProperties::normal()],
        vec![TileProperties::normal(), TileProperties::normal(), TileProperties::normal()],
    ];

    let msg = ExecuteMsg::AddTrack {
        name: "Track One".to_string(), // Duplicate name
        width: 3,
        height: 3,
        layout: duplicate_layout,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_err()); // Should fail due to duplicate name
} 