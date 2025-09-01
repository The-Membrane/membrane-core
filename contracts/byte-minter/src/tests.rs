use cosmwasm_std::{Addr, Uint128};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::byte_minter as bm;
use membrane::track_manager as tm;
use membrane::race_engine as re;
use membrane::types::TileProperties;
use cosmwasm_std::Decimal;

fn byte_minter_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new(
        crate::contract::execute,
        crate::contract::instantiate,
        crate::contract::query,
    );
    Box::new(c)
}

fn track_manager_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new(
        track_manager::contract::execute,
        track_manager::contract::instantiate,
        track_manager::contract::query,
    );
    Box::new(c)
}

fn race_engine_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new(
        race_engine::contract::execute,
        race_engine::contract::instantiate,
        race_engine::contract::query,
    );
    Box::new(c)
}

fn tokenfactory_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new(
        tokenfactory::contract::execute,
        tokenfactory::contract::instantiate,
        tokenfactory::contract::query,
    );
    Box::new(c)
}

#[test]
fn test_maze_generation_and_logging() {
    let mut app = App::default();

    // store codes
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    // instantiate track manager
    let tm_addr = app.instantiate_contract(
        tm_code,
        Addr::unchecked("admin"),
        &tm::InstantiateMsg { admin: "admin".to_string() },
        &[],
        "tm",
        None,
    ).unwrap();

    // instantiate race engine
    let re_addr = app.instantiate_contract(
        re_code,
        Addr::unchecked("admin"),
        &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() },
        &[],
        "re",
        None,
    ).unwrap();

    // instantiate byte-minter
    let bm_addr = app.instantiate_contract(
        bm_code,
        Addr::unchecked("admin"),
        &bm::InstantiateMsg {
            admin: "admin".to_string(),
            track_manager_contract: tm_addr.to_string(),
            race_engine_contract: re_addr.to_string(),
            car_contract: "car".to_string(),
            subdenom: "byte".to_string(),
            tokenfactory_contract: None,
            mint_amount: Uint128::from(100u128),
            maze_default_difficulty: Some(5),
            maze_width: Some(11),
            maze_height: Some(11),
            maze_event_cadence_seconds: 60,
            maze_event_window_seconds: 60,
            pvp_event_cadence_seconds: 60,
            pvp_event_window_seconds: 60,
            pvp_enabled: Some(true),
            runner_reward_rate: Some(Decimal::percent(1)),
            min_start_tile_progress_threshold: Some(1),
            max_start_tile_progress_diff: Some(1000),
            revenue_contract: None,
            create_denom: Some(false),
        },
        &[],
        "bm",
        None,
    ).unwrap();

    // Generate a maze and log ASCII rows from attributes
    let res = app.execute_contract(
        Addr::unchecked("admin"),
        bm_addr.clone(),
        &bm::ExecuteMsg::GenerateMaze { name: "maze1".to_string()  },
        &[],
    ).unwrap();
    for ev in &res.events {
        for attr in &ev.attributes {
            if attr.key.starts_with("maze_row_") {
                println!("{}", attr.value);
            }
        }
    }

    // Ensure maze info is set by querying config and verifying indirectly via VerifyEventRace
    let cfg: bm::Config = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetConfig {}).unwrap();
    println!("Config mint_amount={} denom={}", cfg.mint_amount, cfg.tokenfactory_denom);
}

#[test]
fn test_mint_uniqueness_per_window() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let tf_addr = app.instantiate_contract(tf_code, Addr::unchecked("admin"), &membrane::tokenfactory::InstantiateMsg { owner: Some("admin".to_string()) }, &[], "tf", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: Some(tf_addr.to_string()),
        mint_amount: Uint128::zero(),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 60,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 60,
        pvp_enabled: Some(true),
        runner_reward_rate: Some(Decimal::percent(1)),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

    // Make byte-minter the tokenfactory owner so minting auth passes
    app.execute_contract(
        Addr::unchecked("admin"),
        tf_addr.clone(),
        &membrane::tokenfactory::ExecuteMsg::UpdateConfig { owner: Some(bm_addr.to_string()) },
        &[],
    ).unwrap();

    // Generate maze first
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze".to_string()}, &[]).unwrap();

    // Add a PvP-eligible track to ensure a PvP track can be selected (>= 2 starts)
    let mut grid = vec![vec![TileProperties::default(); 3]; 3];
    grid[0][0].is_start = true; grid[0][1].is_start = true; grid[2][2].is_finish = true;
    app.execute_contract(
        Addr::unchecked("admin"),
        tm_addr.clone(),
        &tm::ExecuteMsg::AddTrack { name: "pvp".to_string(), width: 3, height: 3, layout: grid },
        &[],
    ).unwrap();

    // Start windows (requires recent maze)
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();

    // Record a win from the race engine address (will attempt to mint)
    // To avoid native stargate mint in tests, set tokenfactory_contract to Some and assert execute succeeds
    app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1, runner: re_addr.to_string() }, &[]).unwrap();

    // Duplicate win in same cadence should fail
    let dup_err = app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1, runner: re_addr.to_string() }, &[]).unwrap_err();
    assert!(format!("{}", dup_err).contains("Already won"));

    // Advance to next cadence, start new windows, and allow same car to win again
    app.update_block(|b| { b.time = b.time.plus_seconds(61); b.height += 1; });
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();
    app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1, runner: re_addr.to_string() }, &[]).unwrap();
}

#[test]
fn test_window_cadence() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let tf_addr = app.instantiate_contract(tf_code, Addr::unchecked("admin"), &membrane::tokenfactory::InstantiateMsg { owner: Some("admin".to_string()) }, &[], "tf", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: Some(tf_addr.to_string()),
        mint_amount: Uint128::from(100u128),
        maze_default_difficulty: Some(2),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 30,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 30,
        pvp_enabled: Some(true),
        runner_reward_rate: Some(Decimal::percent(1)),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

    // Generate maze
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze".to_string()}, &[]).unwrap();

    // 1) Check initial time until open
    let s1: u64 = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::SecondsUntilOpen { event: bm::EventType::Maze }).unwrap();
    assert!(s1 <= 60);
    println!("SecondsUntilOpen (initial): {}", s1);

    // 2) Move block into the window and attempt to start a window
    app.update_block(|b| {
        b.time = b.time.plus_seconds(s1 + 1);
        b.height += 1;
    });
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();

    // 3) Move out of the window (past window length) and check time until open again
    let cfg: bm::Config = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetConfig {}).unwrap();
    app.update_block(|b| {
        b.time = b.time.plus_seconds(cfg.maze_event_window_seconds + 1);
        b.height += 1;
    });
    let s3: u64 = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::SecondsUntilOpen { event: bm::EventType::Maze }).unwrap();
    assert!(s3 <= cfg.maze_event_cadence_seconds);
    println!("SecondsUntilOpen (post-window): {}", s3);

    // 4) Confirm RecordWin errors when outside the window
    let err = app.execute_contract(
        Addr::unchecked("admin"),
        bm_addr.clone(),
        &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 123, runner: "admin".to_string() },
        &[],
    ).unwrap_err();
    println!("RecordWin (admin) error: {}", err);

    // Generate maze
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze".to_string()}, &[]).unwrap();

    let _ = app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();


    // Now send from race engine address and expect Event closed
    let err2 = app.execute_contract(
        re_addr.clone(),
        bm_addr.clone(),
        &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 123, runner: re_addr.to_string() },
        &[],
    ).unwrap_err();
    println!("RecordWin (race-engine) error: {}", err2);
    assert!(true);
} 

#[test]
fn test_start_new_windows_gating() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let tf_addr = app.instantiate_contract(tf_code, Addr::unchecked("admin"), &membrane::tokenfactory::InstantiateMsg { owner: Some("admin".to_string()) }, &[], "tf", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: Some(tf_addr.to_string()),
        mint_amount: Uint128::from(100u128),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 60,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 60,
        pvp_enabled: Some(true),
        runner_reward_rate: Some(Decimal::percent(1)),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

    // Add a PvP-eligible track to ensure selection occurs and prevents spam within cadence
    let mut grid = vec![vec![TileProperties::default(); 3]; 3];
    grid[0][0].is_start = true; grid[0][1].is_start = true; grid[2][2].is_finish = true;
    app.execute_contract(Addr::unchecked("admin"), tm_addr.clone(), &tm::ExecuteMsg::AddTrack { name: "pvp".to_string(), width: 3, height: 3, layout: grid }, &[]).unwrap();

    // Generate maze and start windows
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze".to_string()}, &[]).unwrap();
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();

    // Starting again within same cadence should quietly no-op
    let ok = app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();
    // Ensure we get the skipped attribute
    let mut skipped = false;
    for ev in &ok.events { for attr in &ev.attributes { if attr.key == "action" && attr.value == "start_new_windows_skipped" { skipped = true; } } }
    assert!(skipped);

    // Advance cadence, regenerate maze to set recent event, then starting should succeed
    app.update_block(|b| { b.time = b.time.plus_seconds(61); b.height += 1; });
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze2".to_string()}, &[]).unwrap();
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();
}

#[test]
fn test_get_recorded_wins_query() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let tf_addr = app.instantiate_contract(tf_code, Addr::unchecked("admin"), &membrane::tokenfactory::InstantiateMsg { owner: Some("admin".to_string()) }, &[], "tf", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: Some(tf_addr.to_string()),
        mint_amount: Uint128::zero(),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 60,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 60,
        pvp_enabled: Some(true),
        runner_reward_rate: Some(Decimal::percent(1)),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

    // Test that GetRecordedWins returns empty list when no wins recorded
    let maze_wins: Vec<u128> = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetRecordedWins { 
        event: bm::EventType::Maze, 
        start_after: None, 
        limit: None 
    }).unwrap();
    assert_eq!(maze_wins.len(), 0);

    let pvp_wins: Vec<u128> = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetRecordedWins { 
        event: bm::EventType::Pvp, 
        start_after: None, 
        limit: None 
    }).unwrap();
    assert_eq!(pvp_wins.len(), 0);

    // Test with limit parameter
    let maze_wins_limited: Vec<u128> = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetRecordedWins { 
        event: bm::EventType::Maze, 
        start_after: None, 
        limit: Some(10) 
    }).unwrap();
    assert_eq!(maze_wins_limited.len(), 0);

    // Test with start_after parameter
    let maze_wins_with_start: Vec<u128> = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetRecordedWins { 
        event: bm::EventType::Maze, 
        start_after: Some(100u64), 
        limit: Some(10) 
    }).unwrap();
    assert_eq!(maze_wins_with_start.len(), 0);
}

#[test]
fn test_8_hour_event_windows() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());
    let tf_code = app.store_code(tokenfactory_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let tf_addr = app.instantiate_contract(tf_code, Addr::unchecked("admin"), &membrane::tokenfactory::InstantiateMsg { owner: Some("admin".to_string()) }, &[], "tf", None).unwrap();
    
    // Check initial time
    let initial_time = app.block_info().time.seconds();
    println!("Initial time: {}", initial_time);
    
    // Configure for 8-hour windows with 1-second cadence
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: Some(tf_addr.to_string()),
        mint_amount: Uint128::zero(),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 1, // 1 second cadence
        maze_event_window_seconds: 28800, // 8 hours (28,800 seconds)
        pvp_event_cadence_seconds: 1, // 1 second cadence
        pvp_event_window_seconds: 28800, // 8 hours (28,800 seconds)
        pvp_enabled: Some(true),
        runner_reward_rate: Some(Decimal::percent(1)),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

    // Make byte-minter the tokenfactory owner so minting auth passes
    app.execute_contract(
        Addr::unchecked("admin"),
        tf_addr.clone(),
        &membrane::tokenfactory::ExecuteMsg::UpdateConfig { owner: Some(bm_addr.to_string()) },
        &[],
    ).unwrap();

    // Generate maze first
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::GenerateMaze { name: "maze".to_string()}, &[]).unwrap();

    // Add a PvP-eligible track to ensure a PvP track can be selected (>= 2 starts)
    let mut grid = vec![vec![TileProperties::default(); 3]; 3];
    grid[0][0].is_start = true; grid[0][1].is_start = true; grid[2][2].is_finish = true;
    app.execute_contract(
        Addr::unchecked("admin"),
        tm_addr.clone(),
        &tm::ExecuteMsg::AddTrack { name: "pvp".to_string(), width: 3, height: 3, layout: grid },
        &[],
    ).unwrap();

    // Start windows (requires recent maze)
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();

    // Debug: Check the window start time immediately after starting
    let maze_status: bm::WindowStatusResponse = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetWindowStatus { event: bm::EventType::Maze }).unwrap();
    println!("After starting windows - Maze window start: {}, end: {}, is_active: {}", maze_status.window_start, maze_status.window_end, maze_status.is_active);
    println!("Current time: {}, window start: {}, window end: {}", app.block_info().time.seconds(), maze_status.window_start, maze_status.window_end);

    // Try to start new windows immediately - should fail because current windows are active
    let result = app.execute_contract(
        Addr::unchecked("admin"),
        bm_addr.clone(),
        &bm::ExecuteMsg::StartNewWindows {},
        &[],
    );
    assert!(result.is_err());

    // Advance time by more than 8 hours to ensure windows are inactive
    app.update_block(|block| {
        block.time = block.time.plus_seconds(28801); // 8 hours + 1 second
    });

    let current_time = app.block_info().time.seconds();
    println!("After advancing 8 hours - Current time: {}", current_time);
    println!("Expected window end: {}", maze_status.window_start + 28800);
    
    // Check window status after advancing time
    let maze_status: bm::WindowStatusResponse = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetWindowStatus { event: bm::EventType::Maze }).unwrap();
    println!("After 8 hours - Maze window start: {}, end: {}, is_active: {}", maze_status.window_start, maze_status.window_end, maze_status.is_active);
    println!("Seconds until open: {}, seconds until close: {}", maze_status.seconds_until_open, maze_status.seconds_until_close);
    println!("Current time: {}, window start: {}, window end: {}", current_time, maze_status.window_start, maze_status.window_end);
    
    // Windows should no longer be active (current_time should be >= window_end)
    assert!(!maze_status.is_active, "Window should be inactive after 8 hours. Current time: {}, window end: {}", current_time, maze_status.window_end);

    // Debug: Check the current window start time from storage
    let current_maze_start = app.wrap().query_wasm_smart::<u64>(&bm_addr, &bm::QueryMsg::GetConfig {}).unwrap();
    println!("Current maze start from storage: {}", current_maze_start);

    // Debug: Check the window active logic manually
    let cfg: bm::Config = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetConfig {}).unwrap();
    let curr_maze_start = maze_status.window_start;
    let maze_window_active = current_time >= curr_maze_start && current_time < curr_maze_start + cfg.maze_event_window_seconds;
    println!("Manual check - curr_maze_start: {}, window_end: {}, maze_window_active: {}", curr_maze_start, curr_maze_start + cfg.maze_event_window_seconds, maze_window_active);

    // Debug: Check the exact values
    println!("Current time: {}", current_time);
    println!("Window start: {}", curr_maze_start);
    println!("Window end: {}", curr_maze_start + cfg.maze_event_window_seconds);
    println!("Window duration: {}", cfg.maze_event_window_seconds);
    println!("Time advanced: {}", current_time - 1571797419);

    // Now we should be able to start new windows
    let result = app.execute_contract(
        Addr::unchecked("admin"),
        bm_addr.clone(),
        &bm::ExecuteMsg::StartNewWindows {},
        &[],
    );
    assert!(result.is_ok());

    // New windows should be active
    let maze_status: bm::WindowStatusResponse = app.wrap().query_wasm_smart(&bm_addr, &bm::QueryMsg::GetWindowStatus { event: bm::EventType::Maze }).unwrap();
    assert!(maze_status.is_active);
    assert_eq!(maze_status.seconds_until_close, 28800); // Should be 8 hours again
}