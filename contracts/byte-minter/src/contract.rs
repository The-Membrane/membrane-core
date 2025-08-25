use cosmwasm_std::{entry_point, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Response, StdResult, Uint128};
use cw_storage_plus::Bound;
use membrane::byte_minter as bm;
use membrane::tokenfactory::{mint_msg, create_denom_msg};
use membrane::track_manager as tm;
use membrane::types::{TileProperties, Track, TrackTile};
use cw721_base::OwnerOfResponse;

use crate::error::ContractError;
use crate::state::{get_config, set_config, MAZE_EVENT_INFO, MazeEventInfo, CONFIG, MAZE_WINDOW_START, PVP_WINDOW_START, MAZE_WINNERS, PVP_WINNERS, PVP_EVENT_TRACK_ID};

/// Simple deterministic PRNG (LCG)
fn prng(seed: u64, modulus: u32) -> u32 {
    if modulus == 0 { return 0; }
    let a: u64 = 1103515245;
    let c: u64 = 12345;
    ((a.wrapping_mul(seed).wrapping_add(c)) % (modulus as u64)) as u32
}

#[entry_point]
pub fn instantiate(deps: DepsMut, env: Env, info: MessageInfo, msg: bm::InstantiateMsg) -> Result<Response, ContractError> {
    let admin = deps.api.addr_validate(&msg.admin)?;

    // Create denom using tokenfactory. If a tokenfactory contract is set, call it; otherwise emit native MsgCreateDenom
    let should_create = msg.create_denom.unwrap_or(true);
    let create = if should_create {
        Some(create_denom_msg(
            msg.tokenfactory_contract.as_ref().and_then(|s| deps.api.addr_validate(s).ok()),
            info.sender.as_str(),
            &msg.subdenom,
        ))
    } else { None };

    // Full denom per tokenfactory rules: factory/{creator}/{subdenom}
    let full_denom = format!("factory/{}/{}", info.sender, msg.subdenom);

    let cfg = bm::Config {
        admin: admin.to_string(),
        track_manager_contract: msg.track_manager_contract,
        race_engine_contract: msg.race_engine_contract,
        car_contract: msg.car_contract,
        tokenfactory_denom: full_denom,
        tokenfactory_contract: msg.tokenfactory_contract,
        mint_amount: msg.mint_amount,
        maze_default_difficulty: msg.maze_default_difficulty.unwrap_or(2),
        maze_width: msg.maze_width.unwrap_or(15),
        maze_height: msg.maze_height.unwrap_or(15),
        maze_event_cadence_seconds: msg.maze_event_cadence_seconds,
        maze_event_window_seconds: msg.maze_event_window_seconds,
        pvp_event_cadence_seconds: msg.pvp_event_cadence_seconds,
        pvp_event_window_seconds: msg.pvp_event_window_seconds,
        min_progress_to_finish_per_start_tile: msg.min_progress_to_finish_per_start_tile.unwrap_or(1),
        min_start_tile_progress_threshold: msg.min_start_tile_progress_threshold.unwrap_or(1),
        max_start_tile_progress_diff: msg.max_start_tile_progress_diff.unwrap_or(1000),
        revenue_contract: msg.revenue_contract,
    };
    set_config(deps.storage, cfg.clone())?;

    // Initialize event windows to current time rounded down to cadence
    let now = env.block.time.seconds();
    let maze_start = now - (now % cfg.maze_event_cadence_seconds);
    let pvp_start = now - (now % cfg.pvp_event_cadence_seconds);
    MAZE_WINDOW_START.save(deps.storage, &maze_start)?;
    PVP_WINDOW_START.save(deps.storage, &pvp_start)?;

    Ok(Response::new()
        .add_messages(create.into_iter())
        .add_attribute("action", "instantiate")
        .add_attribute("denom", cfg.tokenfactory_denom))
}

#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: bm::ExecuteMsg) -> Result<Response, ContractError> {
    match msg {
        bm::ExecuteMsg::GenerateMaze { name } => exec_generate_maze(deps, env, info, name),
        bm::ExecuteMsg::StartNewWindows {} => exec_start_new_windows(deps, env, info),
        bm::ExecuteMsg::SetEventConfig { maze_cadence_seconds, maze_window_seconds, pvp_cadence_seconds, pvp_window_seconds } => exec_set_event_config(deps, info, maze_cadence_seconds, maze_window_seconds, pvp_cadence_seconds, pvp_window_seconds),
        bm::ExecuteMsg::RecordWin { event, car_id } => exec_record_win(deps, env, info, event, car_id),
        bm::ExecuteMsg::TokenfactoryPassthrough { msgs } => exec_tokenfactory_passthrough(deps, info, msgs),
    }
}

fn assert_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let cfg = get_config(deps.storage)?;
    if info.sender.as_str() != cfg.admin { return Err(ContractError::Unauthorized {}); }
    Ok(())
}

fn assert_revenue_or_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let cfg = get_config(deps.storage)?;
    if info.sender.as_str() == cfg.admin { return Ok(()); }
    if let Some(rc) = cfg.revenue_contract { if info.sender.as_str() == rc { return Ok(()); } }
    Err(ContractError::Unauthorized {})
}

fn exec_tokenfactory_passthrough(deps: DepsMut, info: MessageInfo, msgs: Vec<cosmwasm_std::CosmosMsg>) -> Result<Response, ContractError> {
    assert_revenue_or_admin(&deps, &info)?;
    Ok(Response::new().add_messages(msgs).add_attribute("action", "tokenfactory_passthrough"))
}

fn exec_set_event_config(deps: DepsMut, info: MessageInfo, maze_cad: Option<u64>, maze_win: Option<u64>, pvp_cad: Option<u64>, pvp_win: Option<u64>) -> Result<Response, ContractError> {
    assert_admin(&deps, &info)?;
    let mut cfg = get_config(deps.storage)?;
    if let Some(v) = maze_cad { cfg.maze_event_cadence_seconds = v; }
    if let Some(v) = maze_win { cfg.maze_event_window_seconds = v; }
    if let Some(v) = pvp_cad { cfg.pvp_event_cadence_seconds = v; }
    if let Some(v) = pvp_win { cfg.pvp_event_window_seconds = v; }
    set_config(deps.storage, cfg)?;
    Ok(Response::new().add_attribute("action", "set_event_config"))
}

fn exec_start_new_windows(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    assert_admin(&deps, &info)?;
    let cfg = get_config(deps.storage)?;
    let now = env.block.time.seconds();
    let new_maze_start = now - (now % cfg.maze_event_cadence_seconds);
    let new_pvp_start = now - (now % cfg.pvp_event_cadence_seconds);

    // Load current starts
    let curr_maze_start = MAZE_WINDOW_START.load(deps.storage).unwrap_or(0);
    let curr_pvp_start = PVP_WINDOW_START.load(deps.storage).unwrap_or(0);

    // Anti-spam gating: only allow if cadence advanced OR no PvP selected yet for current cadence
    let already_selected = PVP_EVENT_TRACK_ID.load(deps.storage).unwrap_or(None).is_some();
    let cadence_advanced = new_pvp_start > curr_pvp_start;
    if !(cadence_advanced || !already_selected) {
        return Err(ContractError::InvalidInput("Mint window already started".to_string()));
    }

    // Update window starts only when cadence advances
    if new_maze_start > curr_maze_start { MAZE_WINDOW_START.save(deps.storage, &new_maze_start)?; }
    if new_pvp_start > curr_pvp_start { PVP_WINDOW_START.save(deps.storage, &new_pvp_start)?; }

    // Make a single query for PvP track IDs and pick a valid one from that list
    let list_resp: membrane::track_manager::PvpTrackIdsResponse = deps.querier.query_wasm_smart(
        cfg.track_manager_contract.clone(),
        &tm::QueryMsg::ListPvpTrackIds { start_after: None, limit: Some(1024) },
    )?;
    let mut chosen_pvp: Option<u128> = None;
    if !list_resp.ids.is_empty() {
        // Deterministically choose a starting index, then scan within this one list
        let start_idx = (prng((env.block.height as u64) ^ now, list_resp.ids.len() as u32) as usize) % list_resp.ids.len();
        for i in 0..list_resp.ids.len() {
            let candidate = list_resp.ids[(start_idx + i) % list_resp.ids.len()];
            // Enforce progress threshold across all starting tiles
            let t: membrane::types::Track = deps.querier.query_wasm_smart(
                cfg.track_manager_contract.clone(),
                &tm::QueryMsg::GetTrack { track_id: Uint128::from(candidate) },
            )?;
            let all_ok = t.starting_tiles.iter().all(|st| st.progress_towards_finish >= cfg.min_start_tile_progress_threshold);
            let range_ok = if !t.starting_tiles.is_empty() {
                let min = t.starting_tiles.iter().map(|st| st.progress_towards_finish).min().unwrap();
                let max = t.starting_tiles.iter().map(|st| st.progress_towards_finish).max().unwrap();
                (max - min) <= cfg.max_start_tile_progress_diff
            } else { false };
            if all_ok && range_ok { chosen_pvp = Some(candidate); break; }
        }
    }
    PVP_EVENT_TRACK_ID.save(deps.storage, &chosen_pvp)?;

    // Validate a maze event track has been set recently (within window length)
    let info = MAZE_EVENT_INFO.may_load(deps.storage)?.unwrap_or(MazeEventInfo { track_id: None, set_ts: None });
    let maze_ok = if let (Some(_id), Some(ts)) = (info.track_id, info.set_ts) {
        now.saturating_sub(ts) <= cfg.maze_event_window_seconds
    } else { false };

    if !maze_ok {
        return Err(ContractError::InvalidInput("maze event track not set recently".to_string()));
    }

    Ok(Response::new().add_attribute("action", "start_new_windows"))
}

fn exec_generate_maze(deps: DepsMut, env: Env, info: MessageInfo, name: String) -> Result<Response, ContractError> {
    assert_admin(&deps, &info)?;
    let cfg = get_config(deps.storage)?;
    // Size scaling: allow caller overrides; else use config scalers
    let w = cfg.maze_width as usize;
    let h = cfg.maze_height as usize;
    let diff = cfg.maze_default_difficulty;
    let base_seed = env.block.height as u64;
    let seed_mix = base_seed ^ (((w as u64) << 32) ^ (h as u64));

    // Generate maze layout using difficulty-aware generator
    let mut layout = generate_maze_layout(w, h, diff, seed_mix);
    // Optional sparsity (extra walls) based on difficulty while maintaining solvability
    apply_sparsity(&mut layout, diff, seed_mix)?;

    // Ensure completible within race engine max_ticks
    let re_cfg: membrane::race_engine::Config = deps.querier.query_wasm_smart(
        cfg.race_engine_contract.clone(),
        &membrane::race_engine::QueryMsg::GetConfig {}
    )?;
    let fastest = fastest_steps(&layout).unwrap_or(u64::MAX);
    if fastest as u32 > re_cfg.max_ticks { return Err(ContractError::InvalidInput("maze not completable within max_ticks".to_string())); }

    // Enforce per-start minimum progress and fairness range
    if !starts_meet_min_progress(&layout, cfg.min_progress_to_finish_per_start_tile) {
        return Err(ContractError::InvalidInput("maze progress below minimum per start tile".to_string()));
    }
    if !starts_within_progress_range(&layout, cfg.max_start_tile_progress_diff) {
        return Err(ContractError::InvalidInput("maze start tiles progress range too wide".to_string()));
    }

    // Predict next track id before adding
    let next_id: Uint128 = deps.querier.query_wasm_smart(cfg.track_manager_contract.clone(), &tm::QueryMsg::GetTrackCount {})?;

    // Convert to TileProperties grid
    let tiles: Vec<Vec<TileProperties>> = layout_to_tiles(&layout);

    // Submit to track manager
    let msg = cosmwasm_std::WasmMsg::Execute {
        contract_addr: cfg.track_manager_contract.clone(),
        msg: to_json_binary(&tm::ExecuteMsg::AddTrack { name, width: w as u8, height: h as u8, layout: tiles })?,
        funds: vec![],
    };

    // Mark this maze track as the current event track
    MAZE_EVENT_INFO.save(deps.storage, &MazeEventInfo { track_id: Some(next_id.u128()), set_ts: Some(env.block.time.seconds()) })?;

    let ascii_rows = overlay_shortest_path_ascii(&layout);
    let mut resp = Response::new()
        .add_message(CosmosMsg::Wasm(msg))
        .add_attribute("action", "generate_maze");
    for (i, row) in ascii_rows.iter().enumerate() {
        resp = resp.add_attribute(format!("maze_row_{}", i), row);
    }
    Ok(resp)
}

fn layout_to_tiles(layout: &Vec<Vec<u8>>) -> Vec<Vec<TileProperties>> {
    let h = layout.len();
    let w = if h > 0 { layout[0].len() } else { 0 };
    let mut tiles = vec![vec![TileProperties::default(); w]; h];
    for y in 0..h {
        for x in 0..w {
            let v = layout[y][x];
            let mut t = TileProperties::default();
            if v == 1 { t.blocks_movement = true; } else { t.blocks_movement = false; }
            t.is_start = v == 2; // start
            t.is_finish = v == 3; // finish
            tiles[y][x] = t;
        }
    }
    tiles
}

fn layout_to_ascii_rows(layout: &Vec<Vec<u8>>) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for y in 0..layout.len() {
        let mut s = String::with_capacity(layout[y].len());
        for x in 0..layout[y].len() {
            s.push(match layout[y][x] {
                1 => '#',  // wall
                2 => 'S',  // start
                3 => 'F',  // finish
                _ => ' ',  // path
            });
        }
        rows.push(s);
    }
    rows
}

fn compute_distance_grid(layout: &Vec<Vec<u8>>) -> Option<Vec<Vec<u64>>> {
    use std::collections::VecDeque;
    let h = layout.len(); if h == 0 { return None; }
    let w = layout[0].len(); if w == 0 { return None; }
    let mut dist = vec![vec![u64::MAX; w]; h];
    let mut q = VecDeque::new();
    let mut has_finish = false;
    for y in 0..h { for x in 0..w { if layout[y][x] == 3 { dist[y][x] = 0; q.push_back((x,y)); has_finish = true; } } }
    if !has_finish { return None; }
    while let Some((x,y)) = q.pop_front() {
        let d = dist[y][x];
        for (dx,dy) in [(1,0),(-1,0),(0,1),(0,-1)] {
            let nx = x as i32 + dx; let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
            let (nxu,nyu) = (nx as usize, ny as usize);
            if layout[nyu][nxu] == 1 { continue; }
            if dist[nyu][nxu] == u64::MAX { dist[nyu][nxu] = d + 1; q.push_back((nxu,nyu)); }
        }
    }
    Some(dist)
}

fn overlay_shortest_path_ascii(layout: &Vec<Vec<u8>>) -> Vec<String> {
    let mut rows = layout_to_ascii_rows(layout);
    if let Some(dist) = compute_distance_grid(layout) {
        let h = layout.len(); if h == 0 { return rows; }
        let w = layout[0].len(); if w == 0 { return rows; }
        // Find a start tile
        let mut start: Option<(usize,usize)> = None;
        'outer: for y in 0..h { for x in 0..w { if layout[y][x] == 2 { start = Some((x,y)); break 'outer; } } }
        if let Some((mut x, mut y)) = start {
            // Walk towards finish decreasing distance
            if dist[y][x] != u64::MAX {
                while dist[y][x] > 0 {
                    // mark current as path unless it's S or F
                    let ch = rows[y].as_bytes()[x] as char;
                    if ch != 'S' && ch != 'F' { rows[y].replace_range(x..x+1, "*"); }
                    // pick neighbor with smaller dist
                    let mut moved = false;
                    for (dx,dy) in [(1,0),(-1,0),(0,1),(0,-1)] {
                        let nx = x as i32 + dx; let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
                        let (nxu, nyu) = (nx as usize, ny as usize);
                        if dist[nyu][nxu] < dist[y][x] { x = nxu; y = nyu; moved = true; break; }
                    }
                    if !moved { break; }
                }
                // mark finish
                let ch = rows[y].as_bytes()[x] as char;
                if ch != 'S' && ch != 'F' { rows[y].replace_range(x..x+1, "*"); }
            }
        }
    }
    rows
}

// Maze values: 1 = wall, 0 = path, 2 = start, 3 = finish
fn generate_maze_layout(width: usize, height: usize, difficulty: u8, seed: u64) -> Vec<Vec<u8>> {
    // Ensure odd dimensions for proper maze
    let w = if width % 2 == 0 { width - 1 } else { width };
    let h = if height % 2 == 0 { height - 1 } else { height };
    let mut grid = vec![vec![1u8; w]; h];

    // Carve starting at (1,1)
    carve_biased(1, 1, &mut grid, difficulty, seed);

    // Place start and finish
    grid[1][1] = 2;
    grid[h-2][w-2] = 3;
    grid
}

fn carve_biased(x: usize, y: usize, grid: &mut Vec<Vec<u8>>, difficulty: u8, seed: u64) {
    grid[y][x] = 0;
    // Base directions
    let mut dirs = vec![(2i32,0i32),(-2,0),(0,2),(0,-2)];
    // Shuffle directions deterministically
    for i in 0..dirs.len() {
        let len = dirs.len();
        let j = (prng(seed.wrapping_add(i as u64), len as u32) as usize) % len;
        if i != j { dirs.swap(i, j); }
    }
    // Higher difficulty: reduce branching by stopping after first successful carve
    let stop_after_first = difficulty >= 7;
    for (dx,dy) in dirs.clone() {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if ny <= 0 || nx <= 0 { continue; }
        let (nxu, nyu) = (nx as usize, ny as usize);
        if nyu+1 >= grid.len() || nxu+1 >= grid[0].len() { continue; }
        if grid[nyu][nxu] == 1 {
            // Bias: for higher difficulty, prefer straight corridors by not re-shuffling nested calls
            grid[(y as i32 + dy/2) as usize][(x as i32 + dx/2) as usize] = 0;
            carve_biased(nxu, nyu, grid, difficulty, seed.wrapping_add(((nxu as u64) << 16) ^ (nyu as u64)));
            if stop_after_first { break; }
        }
    }
}

// Increase sparsity by adding walls according to difficulty, while maintaining solvability
fn apply_sparsity(grid: &mut Vec<Vec<u8>>, difficulty: u8, seed: u64) -> Result<(), ContractError> {
    if difficulty == 0 { return Ok(()); }
    let h = grid.len(); if h == 0 { return Ok(()); }
    let w = grid[0].len(); if w == 0 { return Ok(()); }
    // Target number of extra walls roughly proportional to difficulty and area
    let area = (w * h) as u32;
    let mut target = ((area as u64 * (difficulty as u64)) / 512) as u32; // conservative
    if target == 0 { return Ok(()); }
    let mut s = seed;
    let mut attempts = 0u32;
    while target > 0 && attempts < area {
        let rx = prng(s, w as u32) as usize; s = s.wrapping_add(1);
        let ry = prng(s, h as u32) as usize; s = s.wrapping_add(1);
        // Skip borders, start/finish
        if rx <= 0 || ry <= 0 || rx+1 >= w || ry+1 >= h { attempts += 1; continue; }
        if grid[ry][rx] != 0 { attempts += 1; continue; }
        if (ry == 1 && rx == 1) || (ry == h-2 && rx == w-2) { attempts += 1; continue; }
        // Tentatively add wall
        grid[ry][rx] = 1;
        // Keep solvable: there must be a path from start(1,1) to finish(h-2,w-2)
        if fastest_steps(grid).is_none() {
            // revert
            grid[ry][rx] = 0;
        } else {
            target -= 1;
        }
        attempts += 1;
    }
    Ok(())
}

// BFS to compute fastest steps from any start to any finish
fn fastest_steps(layout: &Vec<Vec<u8>>) -> Option<u64> {
    use std::collections::VecDeque;
    let h = layout.len(); if h == 0 { return None; }
    let w = layout[0].len(); if w == 0 { return None; }
    let mut dist = vec![vec![u64::MAX; w]; h];
    let mut q = VecDeque::new();
    for y in 0..h { for x in 0..w { if layout[y][x] == 3 { dist[y][x] = 0; q.push_back((x,y)); } } }
    while let Some((x,y)) = q.pop_front() {
        let d = dist[y][x];
        let dirs = [(1i32,0i32),(-1,0),(0,1),(0,-1)];
        for (dx,dy) in dirs {
            let nx = x as i32 + dx; let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
            let (nxu,nyu) = (nx as usize, ny as usize);
            if layout[nyu][nxu] == 1 { continue; }
            if dist[nyu][nxu] == u64::MAX { dist[nyu][nxu] = d + 1; q.push_back((nxu,nyu)); }
        }
    }
    let mut best = u64::MAX;
    for y in 0..h { for x in 0..w { if layout[y][x] == 2 { if dist[y][x] < best { best = dist[y][x]; } } } }
    if best == u64::MAX { None } else { Some(best) }
}

// Ensure all start tiles (value 2) have distance to finish >= min
fn starts_meet_min_progress(layout: &Vec<Vec<u8>>, min: u16) -> bool {
    use std::collections::VecDeque;
    let h = layout.len(); if h == 0 { return false; }
    let w = layout[0].len(); if w == 0 { return false; }
    let mut dist = vec![vec![u64::MAX; w]; h];
    let mut q = VecDeque::new();
    for y in 0..h { for x in 0..w { if layout[y][x] == 3 { dist[y][x] = 0; q.push_back((x,y)); } } }
    while let Some((x,y)) = q.pop_front() {
        let d = dist[y][x];
        for (dx,dy) in [(1,0),(-1,0),(0,1),(0,-1)] {
            let nx = x as i32 + dx; let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
            let (nxu,nyu) = (nx as usize, ny as usize);
            if layout[nyu][nxu] == 1 { continue; }
            if dist[nyu][nxu] == u64::MAX { dist[nyu][nxu] = d + 1; q.push_back((nxu,nyu)); }
        }
    }
    for y in 0..h { for x in 0..w { if layout[y][x] == 2 { if dist[y][x] == u64::MAX || dist[y][x] < min as u64 { return false; } } } }
    true
}

// Ensure the difference (max - min) of start tiles' distances to finish is within bound
fn starts_within_progress_range(layout: &Vec<Vec<u8>>, max_diff: u16) -> bool {
    use std::collections::VecDeque;
    let h = layout.len(); if h == 0 { return false; }
    let w = layout[0].len(); if w == 0 { return false; }
    let mut dist = vec![vec![u64::MAX; w]; h];
    let mut q = VecDeque::new();
    for y in 0..h { for x in 0..w { if layout[y][x] == 3 { dist[y][x] = 0; q.push_back((x,y)); } } }
    while let Some((x,y)) = q.pop_front() {
        let d = dist[y][x];
        for (dx,dy) in [(1,0),(-1,0),(0,1),(0,-1)] {
            let nx = x as i32 + dx; let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { continue; }
            let (nxu,nyu) = (nx as usize, ny as usize);
            if layout[nyu][nxu] == 1 { continue; }
            if dist[nyu][nxu] == u64::MAX { dist[nyu][nxu] = d + 1; q.push_back((nxu,nyu)); }
        }
    }
    let mut minv = u64::MAX;
    let mut maxv = 0u64;
    let mut found = false;
    for y in 0..h { for x in 0..w { if layout[y][x] == 2 {
        let d = dist[y][x];
        if d == u64::MAX { return false; }
        if d < minv { minv = d; }
        if d > maxv { maxv = d; }
        found = true;
    } } }
    if !found { return false; }
    (maxv - minv) <= (max_diff as u64)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: bm::QueryMsg) -> StdResult<Binary> {
    match msg {
        bm::QueryMsg::VerifyEventRace { track_id, car_ids, pvp } => to_json_binary(&query_verify_event_race(deps, env, track_id, car_ids, pvp)?),
        bm::QueryMsg::GetConfig {} => to_json_binary(&CONFIG.load(deps.storage)?),
        bm::QueryMsg::SecondsUntilOpen { event } => to_json_binary(&seconds_until_open(deps, env, event)?),
    }
}

fn is_within_window(now: u64, start: u64, window: u64) -> bool { now >= start && now < start + window }

fn query_verify_event_race(deps: Deps, env: Env, track_id: u128, car_ids: Vec<u128>, pvp: bool) -> StdResult<bm::VerifyEventRaceResponse> {
    let cfg = get_config(deps.storage)?;
    let now = env.block.time.seconds();
    let maze_start = MAZE_WINDOW_START.load(deps.storage).unwrap_or(0);
    let pvp_start = PVP_WINDOW_START.load(deps.storage).unwrap_or(0);

    if pvp {
        let two_cars_with_zero = car_ids.len() == 2 && car_ids.iter().filter(|&&id| id == 0).count() == 1;
        let allowed = is_within_window(now, pvp_start, cfg.pvp_event_window_seconds) && two_cars_with_zero;
        // Enforce selected PVP track id
        let selected = PVP_EVENT_TRACK_ID.load(deps.storage).unwrap_or(None);
        if let Some(sel) = selected { if sel != track_id { return Ok(bm::VerifyEventRaceResponse { allowed: false, event: None, required_opponent: Some(0) }); } }
        Ok(bm::VerifyEventRaceResponse { allowed, event: if allowed { Some(bm::EventType::Pvp) } else { None }, required_opponent: Some(0) })
    } else {
        // If maze event has an active track, ensure it matches; else allow any maze track
        let allowed = is_within_window(now, maze_start, cfg.maze_event_window_seconds);
        let info = MAZE_EVENT_INFO.may_load(deps.storage)?.unwrap_or(MazeEventInfo { track_id: None, set_ts: None });
        if let Some(sel) = info.track_id { if sel != track_id { return Ok(bm::VerifyEventRaceResponse { allowed: false, event: None, required_opponent: None }); } }
        Ok(bm::VerifyEventRaceResponse { allowed, event: if allowed { Some(bm::EventType::Maze) } else { None }, required_opponent: None })
    }
}

fn seconds_until_open(deps: Deps, env: Env, event: bm::EventType) -> StdResult<u64> {
    let cfg = get_config(deps.storage)?;
    let now = env.block.time.seconds();
    let (cadence, _start) = match event {
        bm::EventType::Maze => (cfg.maze_event_cadence_seconds, MAZE_WINDOW_START.load(deps.storage).unwrap_or(0)),
        bm::EventType::Pvp => (cfg.pvp_event_cadence_seconds, PVP_WINDOW_START.load(deps.storage).unwrap_or(0)),
    };
    let current_window_start = now - (now % cadence);
    let next_window_start = current_window_start + cadence;
    Ok(if now < current_window_start { current_window_start - now } else { next_window_start.saturating_sub(now) })
}

fn exec_record_win(deps: DepsMut, env: Env, info: MessageInfo, event: bm::EventType, car_id: u128) -> Result<Response, ContractError> {
    let cfg = get_config(deps.storage)?;
    if info.sender.as_str() != cfg.race_engine_contract { return Err(ContractError::Unauthorized {}); }

    let (start_key, winners_map) = match event {
        bm::EventType::Maze => (MAZE_WINDOW_START, MAZE_WINNERS),
        bm::EventType::Pvp => (PVP_WINDOW_START, PVP_WINNERS),
    };
    let start = start_key.load(deps.storage).unwrap_or(0);

    if winners_map.may_load(deps.storage, (start, car_id))?.unwrap_or(false) { return Err(ContractError::AlreadyWon {}); }
    winners_map.save(deps.storage, (start, car_id), &true)?;

    // Query car owner from cw721-like contract]
    let owner_resp: OwnerOfResponse = deps.querier.query_wasm_smart(
        cfg.car_contract.clone(),
        &membrane::car::QueryMsg::Base(membrane::car::Cw721QueryMsg::OwnerOf{ token_id: car_id.to_string(), include_expired: None })
    )?;

    let mint = mint_msg(
        cfg.tokenfactory_contract.as_ref().and_then(|s| deps.api.addr_validate(s).ok()),
        &cfg.admin,
        &cfg.tokenfactory_denom,
        cfg.mint_amount,
        &owner_resp.owner,
    );

    Ok(Response::new().add_message(mint).add_attribute("action", "record_win").add_attribute("event", match event { bm::EventType::Maze => "maze", bm::EventType::Pvp => "pvp" }))
} 