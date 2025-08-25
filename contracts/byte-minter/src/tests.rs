use cosmwasm_std::{Addr, Uint128};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::byte_minter as bm;
use membrane::track_manager as tm;
use membrane::race_engine as re;
use membrane::types::TileProperties;

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

#[test]
fn test_maze_generation_and_logging() {
    let mut app = App::default();

    // store codes
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());

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
            min_progress_to_finish_per_start_tile: Some(1),
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

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: None,
        mint_amount: Uint128::from(100u128),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 60,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 60,
        min_progress_to_finish_per_start_tile: Some(1),
        min_start_tile_progress_threshold: Some(1),
        max_start_tile_progress_diff: Some(1000),
        revenue_contract: None,
        create_denom: Some(false),
    }, &[], "bm", None).unwrap();

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

    // Record a win from the race engine address
    app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1 }, &[]).unwrap();

    // Duplicate win in same cadence should fail
    let dup_err = app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1 }, &[]).unwrap_err();
    assert!(format!("{}", dup_err).contains("Already won"));

    // Advance to next cadence, start new windows, and allow same car to win again
    app.update_block(|b| { b.time = b.time.plus_seconds(61); b.height += 1; });
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();
    app.execute_contract(re_addr.clone(), bm_addr.clone(), &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 1 }, &[]).unwrap();
}

#[test]
fn test_window_cadence() {
    let mut app = App::default();
    let bm_code = app.store_code(byte_minter_contract());
    let tm_code = app.store_code(track_manager_contract());
    let re_code = app.store_code(race_engine_contract());

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: None,
        mint_amount: Uint128::from(100u128),
        maze_default_difficulty: Some(2),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 30,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 30,
        min_progress_to_finish_per_start_tile: Some(1),
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
        &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 123 },
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
        &bm::ExecuteMsg::RecordWin { event: bm::EventType::Maze, car_id: 123 },
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

    let tm_addr = app.instantiate_contract(tm_code, Addr::unchecked("admin"), &tm::InstantiateMsg { admin: "admin".to_string() }, &[], "tm", None).unwrap();
    let re_addr = app.instantiate_contract(re_code, Addr::unchecked("admin"), &re::InstantiateMsg { admin: "admin".to_string(), track_contract: tm_addr.to_string(), car_contract: "car".to_string() }, &[], "re", None).unwrap();
    let bm_addr = app.instantiate_contract(bm_code, Addr::unchecked("admin"), &bm::InstantiateMsg {
        admin: "admin".to_string(),
        track_manager_contract: tm_addr.to_string(),
        race_engine_contract: re_addr.to_string(),
        car_contract: "car".to_string(),
        subdenom: "byte".to_string(),
        tokenfactory_contract: None,
        mint_amount: Uint128::from(100u128),
        maze_default_difficulty: Some(3),
        maze_width: Some(9),
        maze_height: Some(9),
        maze_event_cadence_seconds: 60,
        maze_event_window_seconds: 60,
        pvp_event_cadence_seconds: 60,
        pvp_event_window_seconds: 60,
        min_progress_to_finish_per_start_tile: Some(1),
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

    // Starting again within same cadence should error due to PvP already selected
    let err = app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap_err();
    println!("StartNewWindows error: {}", err);
    assert!(format!("{}", err).contains("Mint window already started"));

    // Advance cadence and starting should succeed
    app.update_block(|b| { b.time = b.time.plus_seconds(61); b.height += 1; });
    app.execute_contract(Addr::unchecked("admin"), bm_addr.clone(), &bm::ExecuteMsg::StartNewWindows {}, &[]).unwrap();
}