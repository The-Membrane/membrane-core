
// track_manager/src/contract.rs

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult, Uint128
};
use cw_storage_plus::Bound;
use racing::race_engine::DEFAULT_SPEED;

use crate::error::TrackManagerError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{get_track, set_track, ADMIN, TRACKS, TRACK_ID_COUNTER};
use racing::types::{Track, TrackTile, TileProperties};

const MAX_LIMIT: u32 = 32;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TrackManagerError> {
    let admin = deps.api.addr_validate(&msg.admin)?;
    ADMIN.save(deps.storage, &admin)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", admin))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, TrackManagerError> {
    match msg {
        ExecuteMsg::AddTrack {
            name,
            width,
            height,
            layout,
        } => execute_add_track(deps, _info, name, width, height, layout),
    }
}

pub fn execute_add_track(
    deps: DepsMut,
    _info: MessageInfo,
    name: String,
    width: u8,
    height: u8,
    layout: Vec<Vec<TileProperties>>,
) -> Result<Response, TrackManagerError> {
    // Validate track dimensions
    if width == 0 || height == 0 {
        return Err(TrackManagerError::InvalidTrackDimensions { width, height });
    }

    //Generate a new track id
    let track_id = TRACK_ID_COUNTER.load(deps.storage)?;
    TRACK_ID_COUNTER.save(deps.storage, &(track_id + Uint128::one()))?;

    // Check if track already exists
    // if get_track(deps.storage, &track_id).is_ok() {
    //     return Err(TrackManagerError::TrackAlreadyExists { track_id: track_id.clone() });
    // }

    // Validate layout dimensions
    if layout.len() != height as usize {
        return Err(TrackManagerError::InvalidTrackDimensions { width, height });
    }

    for row in &layout {
        if row.len() != width as usize {
            return Err(TrackManagerError::InvalidTrackDimensions { width, height });
        }
    }

    // Validate track layout
    validate_track_layout(&layout, width, height)?;

    // Calculate progress_towards_finish using A* pathfinding
    let (track_layout, fastest_tick_time) = calculate_progress_towards_finish(&layout, width, height);

    // Calculate track statistics
    let stats = calculate_track_statistics(&layout, width, height);

    let track = Track {
        creator: _info.sender.to_string(),
        id: track_id.into(),
        name,
        width,
        height,
        layout: track_layout,
        fastest_tick_time,
    };

    set_track(deps.storage, &track_id.into(), track)?;

    Ok(Response::new()
        .add_attribute("method", "add_track")
        .add_attribute("track_id", track_id)
        .add_attribute("width", width.to_string())
        .add_attribute("height", height.to_string())
        .add_attribute("finish_tiles", stats.finish_tiles.to_string())
        .add_attribute("boost_tiles", stats.boost_tiles.to_string())
        .add_attribute("slow_tiles", stats.slow_tiles.to_string())
        .add_attribute("stick_tiles", stats.stick_tiles.to_string())
        .add_attribute("wall_tiles", stats.wall_tiles.to_string()))
}

/// Track statistics for validation and analysis
struct TrackStats {
    finish_tiles: u32,
    boost_tiles: u32,
    slow_tiles: u32,
    stick_tiles: u32,
    wall_tiles: u32,
    normal_tiles: u32,
}

/// Calculate statistics for a track layout
fn calculate_track_statistics(
    layout: &Vec<Vec<TileProperties>>,
    width: u8,
    height: u8,
) -> TrackStats {
    let mut stats = TrackStats {
        finish_tiles: 0,
        boost_tiles: 0,
        slow_tiles: 0,
        stick_tiles: 0,
        wall_tiles: 0,
        normal_tiles: 0,
    };

    for y in 0..height {
        for x in 0..width {
            let tile = &layout[y as usize][x as usize];
            if tile.is_finish {
                stats.finish_tiles += 1;
            } else if tile.speed_modifier > DEFAULT_SPEED.into() {
                stats.boost_tiles += 1;
            } else if tile.speed_modifier < DEFAULT_SPEED.into() {
                stats.slow_tiles += 1;
            } else if tile.skip_next_turn {
                stats.stick_tiles += 1;
            } else if tile.blocks_movement {
                stats.wall_tiles += 1;
            } else {
                stats.normal_tiles += 1;
            }
        }
    }

    stats
}

/// Validate track layout for basic requirements
fn validate_track_layout(
    layout: &Vec<Vec<TileProperties>>,
    width: u8,
    height: u8,
) -> Result<(), TrackManagerError> {
    // Check for at least one finish tile
    let has_finish = layout.iter().any(|row| row.iter().any(|tile| tile.is_finish));
    if !has_finish {
        return Err(TrackManagerError::NoFinishTile {});
    }

    // Check for at least one start tile
    let has_start = layout.iter().any(|row| row.iter().any(|tile| tile.is_start));
    if !has_start {
        return Err(TrackManagerError::NoStartTile {});
    }

    // Combined validation and distance calculation
    let distances = calculate_distances_and_validate(layout, width, height)?;
    
    // Check that all start tiles are reachable (distance < u16::MAX)
    for y in 0..height {
        for x in 0..width {
            if layout[y as usize][x as usize].is_start {
                if distances[y as usize][x as usize] == u16::MAX {
                    return Err(TrackManagerError::NoAccessiblePath {});
                }
            }
        }
    }

    // // Additional validation: ensure track is not too small or too large

    //Leave this commented out bc we don't know if small tracks could be useful for training
    // if width < 3 || height < 3 {
    //     return Err(TrackManagerError::TrackTooSmall { width, height });
    // }

    //Leave this commented out bc we don't know how big the track can be
    // if width > 50 || height > 50 {
    //     return Err(TrackManagerError::TrackTooLarge { width, height });
    // }

    Ok(())
}

/// Combined distance calculation and validation using multi-source BFS
/// This replaces both the separate validation and A* distance calculation
fn calculate_distances_and_validate(
    layout: &Vec<Vec<TileProperties>>,
    width: u8,
    height: u8,
) -> Result<Vec<Vec<u16>>, TrackManagerError> {
    use std::collections::VecDeque;
    
    let mut distances = vec![vec![u16::MAX; width as usize]; height as usize];
    let mut queue = VecDeque::new();
    
    // Find all finish tiles and add them to the queue with distance 0
    for y in 0..height {
        for x in 0..width {
            if layout[y as usize][x as usize].is_finish {
                distances[y as usize][x as usize] = 0;
                queue.push_back((x, y));
            }
        }
    }
    
    // Multi-source BFS from all finish tiles
    while let Some((x, y)) = queue.pop_front() {
        let current_distance = distances[y as usize][x as usize];
        
        // Check all 4 directions
        let directions = [(0, 1), (0, -1), (1, 0), (-1, 0)];
        for (dx, dy) in directions {
            let nx = x as i8 + dx;
            let ny = y as i8 + dy;
            
            // Check bounds
            if nx < 0 || ny < 0 || nx >= width as i8 || ny >= height as i8 {
                continue;
            }
            
            let nx = nx as u8;
            let ny = ny as u8;
            
            // Skip if already visited or if tile blocks movement
            if distances[ny as usize][nx as usize] != u16::MAX || 
               layout[ny as usize][nx as usize].blocks_movement {
                continue;
            }
            
            // Update distance and add to queue
            distances[ny as usize][nx as usize] = current_distance + 1;
            queue.push_back((nx, ny));
        }
    }
    
    Ok(distances)
}

/// Calculate progress towards finish for each tile using combined validation and distance calculation
fn calculate_progress_towards_finish(
    layout: &Vec<Vec<TileProperties>>,
    width: u8,
    height: u8,
) -> (Vec<Vec<TrackTile>>, u64) {
    // Use combined distance calculation and validation
    let distances = calculate_distances_and_validate(layout, width, height)
        .expect("Track validation should have passed");
    
    //Save starting tiles 
    let mut starting_tiles = vec![];

    // Convert to TrackTile format
    let mut track_layout = vec![];
    for y in 0..height {
        let mut row = vec![];
        for x in 0..width {
            let properties = layout[y as usize][x as usize].clone();
            let distance = distances[y as usize][x as usize];

            let tile = TrackTile {
                properties: properties.clone(),
                progress_towards_finish: distance,
                x,
                y,
            };

            if properties.is_start {
                starting_tiles.push(tile.clone());
            }
            
            row.push(tile);
        }
        track_layout.push(row);
    }

    let mut fastest = u64::MAX;
    //Find the fastest path from any starting tile
    for start_tile in starting_tiles {
        if (start_tile.progress_towards_finish as u64) < fastest {
            fastest = start_tile.progress_towards_finish as u64;
        }
    }
    
    (track_layout, fastest)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetTrack { track_id } => to_json_binary(&query_get_track(deps, track_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::ListTracks {
            start_after,
            limit,
        } => to_json_binary(&query_list_tracks(deps, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}

pub fn query_get_track(deps: Deps, track_id: Uint128) -> Result<Track, TrackManagerError> {
    let track = get_track(deps.storage, &track_id.into())?;
    
    Ok(track)
}

pub fn query_list_tracks(deps: Deps, start_after: Option<u128>, limit: Option<u32>) -> Result<crate::msg::ListTracksResponse, TrackManagerError> {
    let mut tracks = vec![];
    let start_after = start_after.unwrap_or(0);
    let limit = limit.unwrap_or(MAX_LIMIT);

    for item in TRACKS
        .range(deps.storage, Some(Bound::exclusive(start_after)), None, Order::Ascending)
        .take(limit as usize) {
        let (track_id, track) = item?;
        tracks.push(track);
    }
    Ok(crate::msg::ListTracksResponse { tracks })
}
