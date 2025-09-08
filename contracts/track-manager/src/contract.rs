
// track_manager/src/contract.rs

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult, Uint128
};
use cw_storage_plus::Bound;
use membrane::race_engine::DEFAULT_SPEED;
use membrane::track_manager::MigrateMsg;
use sha2::{Sha256, Digest};

use crate::error::TrackManagerError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{get_track, set_track, ADMIN, TRACKS, TRACK_ID_COUNTER, PVP_TRACK_IDS, save_track_hash, has_track_hash, save_track_id_hash_mapping, get_track_layout_hash, TRACK_LAYOUT_HASHES, TRACK_ID_TO_HASH, save_track_name_hash, has_track_name_hash, save_track_id_name_hash_mapping, get_track_name_hash, TRACK_NAME_HASHES, TRACK_ID_TO_NAME_HASH};
use membrane::types::{Track, TrackTile, TileProperties};

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

    //Set the track id counter to 0
    TRACK_ID_COUNTER.save(deps.storage, &Uint128::zero())?;

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
        ExecuteMsg::RecomputeProgress { track_id } => execute_recompute_progress(deps, track_id),
        ExecuteMsg::EditTrack {
            track_id,
            name,
            delete,
        } => execute_edit_track(deps, _info, track_id, name, delete),
    }
}

pub fn execute_add_track(
    deps: DepsMut,
    _info: MessageInfo,
    name: String,
    mut width: u8,
    mut height: u8,
    layout: Vec<Vec<TileProperties>>,
) -> Result<Response, TrackManagerError> {
    // Validate track dimensions
    if width == 0 || height == 0 {
        return Err(TrackManagerError::InvalidTrackDimensions { width, height });
    }

    // Check for duplicate layout using hash
    let layout_hash = calculate_layout_hash(&layout);
    if has_track_hash(deps.storage, &layout_hash)? {
        return Err(TrackManagerError::DuplicateTrackLayout {});
    }

    // Check for duplicate name using hash
    let name_hash = calculate_name_hash(&name);
    if has_track_name_hash(deps.storage, &name_hash)? {
        return Err(TrackManagerError::DuplicateTrackName {});
    }

    //Generate a new track id
    let track_id = TRACK_ID_COUNTER.load(deps.storage)?;
    TRACK_ID_COUNTER.save(deps.storage, &(track_id + Uint128::one()))?;

    // Check if track already exists
    // if get_track(deps.storage, &track_id).is_ok() {
    //     return Err(TrackManagerError::TrackAlreadyExists { track_id: track_id.clone() });
    // }

    //Set track width and height
    width = layout[0].len() as u8;
    height = layout.len() as u8;
    
    // Validate track layout
    validate_track_layout(&layout, width, height)?;

    // Calculate progress_towards_finish using A* pathfinding
    let (track_layout, fastest_tick_time, starting_tiles) = calculate_progress_towards_finish(
        &layout, 
        width, 
        height
    );

    // Calculate track statistics
    let stats = calculate_track_statistics(&layout, width, height);

    // Create full track for race engine compatibility (stored directly)
    let track = Track {
        creator: _info.sender.to_string(),
        id: track_id.into(),
        name,
        width,
        height,
        layout: track_layout,
        fastest_tick_time,
        starting_tiles,
    };

    set_track(deps.storage, &track_id.into(), track)?;

    // Save the layout hash to prevent duplicates
    save_track_hash(deps.storage, &layout_hash, &track_id.into())?;
    
    // Save the reverse mapping for queries
    save_track_id_hash_mapping(deps.storage, &track_id.into(), &layout_hash)?;

    // Save the name hash to prevent duplicates
    save_track_name_hash(deps.storage, &name_hash, &track_id.into())?;
    
    // Save the reverse mapping for name queries
    save_track_id_name_hash_mapping(deps.storage, &track_id.into(), &name_hash)?;

    // Mark PvP-eligible tracks (>=2 starting tiles)
    if stats.starting_tiles >= 2 { PVP_TRACK_IDS.save(deps.storage, track_id.u128(), &true)?; }

    Ok(Response::new()
        .add_attribute("method", "add_track")
        .add_attribute("track_id", track_id)
        .add_attribute("width", width.to_string())
        .add_attribute("height", height.to_string())
        .add_attribute("finish_tiles", stats.finish_tiles.to_string())
        .add_attribute("boost_tiles", stats.boost_tiles.to_string())
        .add_attribute("slow_tiles", stats.slow_tiles.to_string())
        .add_attribute("stick_tiles", stats.stick_tiles.to_string())
        .add_attribute("wall_tiles", stats.wall_tiles.to_string())
        .add_attribute("layout_hash", layout_hash))
}

/// Track statistics for validation and analysis
struct TrackStats {
    finish_tiles: u32,
    boost_tiles: u32,
    slow_tiles: u32,
    stick_tiles: u32,
    wall_tiles: u32,
    normal_tiles: u32,
    starting_tiles: u32,
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
        starting_tiles: 0,
    };

    for y in 0..height {
        for x in 0..width {
            let tile = &layout[y as usize][x as usize];
            if tile.is_finish {
                stats.finish_tiles += 1;
            } else if tile.is_start {
                stats.starting_tiles += 1;
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
) -> (Vec<Vec<TrackTile>>, u64, Vec<TrackTile>) {
    // Use combined distance calculation and validation
    let distances = calculate_distances_and_validate(layout, width, height)
        .expect("Track validation should have passed");
    
    //Save starting tiles 
    let mut starting_tiles = vec![];

    // Convert to TrackTile format
    let mut track_layout = vec![];
    // Compute minimum steps to finish among all starting tiles (for normalization)
    let mut min_start_steps: u16 = u16::MAX;
    for y in 0..height {
        for x in 0..width {
            let properties = layout[y as usize][x as usize].clone();
            if properties.is_start {
                let d = distances[y as usize][x as usize];
                if d < min_start_steps { min_start_steps = d; }
            }
        }
    }
    for y in 0..height {
        let mut row = vec![];
        for x in 0..width {
            let properties = layout[y as usize][x as usize].clone();
            let distance = distances[y as usize][x as usize];

            // New semantics: progress increases as you get closer to finish.
            // Normalize so the best (minimum) start tile has 0 and finish has max value.
            let mut progress: u16 = 0;
            if !properties.blocks_movement && distance != u16::MAX && min_start_steps != u16::MAX {
                // Clamp to zero to avoid underflow when tile is farther than best start
                if distance <= min_start_steps {
                    progress = min_start_steps - distance;
                } else {
                    progress = 0;
                }
            }

            let mut tile = TrackTile {
                properties: properties.clone(),
                progress_towards_finish: progress,
                min_steps_to_finish_from_start: None,
                x,
                y,
            };

            if properties.is_start {
                tile.min_steps_to_finish_from_start = if distance != u16::MAX { Some(distance) } else { None };
                starting_tiles.push(tile.clone());
            }
            
            row.push(tile);
        }
        track_layout.push(row);
    }

    let mut fastest = u64::MAX;
    //Find the fastest path from any starting tile
    for start_tile in &starting_tiles {
        if let Some(steps) = start_tile.min_steps_to_finish_from_start {
            if (steps as u64) < fastest { fastest = steps as u64; }
        }
    }
    
    (track_layout, fastest, starting_tiles)
}

/// Calculate SHA-256 hash of track layout for duplicate detection
fn calculate_layout_hash(layout: &Vec<Vec<TileProperties>>) -> String {
    let mut hasher = Sha256::new();
    
    // Serialize the layout in a deterministic way
    for row in layout {
        for tile in row {
            // Convert tile properties to bytes in a consistent order
            hasher.update(&tile.speed_modifier.to_le_bytes());
            hasher.update(&[tile.blocks_movement as u8]);
            hasher.update(&[tile.skip_next_turn as u8]);
            hasher.update(&tile.damage.to_le_bytes());
            hasher.update(&[tile.is_finish as u8]);
            hasher.update(&[tile.is_start as u8]);
        }
    }
    
    // Return hex string of the hash
    format!("{:x}", hasher.finalize())
}

/// Calculate SHA-256 hash of track name for duplicate detection
fn calculate_name_hash(name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetTrack { track_id } => to_json_binary(&query_get_track(deps, track_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::ListTracks {
            start_after,
            limit,
        } => to_json_binary(&query_list_tracks(deps, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTrackCount {} => to_json_binary(&TRACK_ID_COUNTER.load(deps.storage)?),
        QueryMsg::ListPvpTrackIds { start_after, limit } => to_json_binary(&query_list_pvp_track_ids(deps, start_after, limit).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::HasLayoutHash { layout_hash } => to_json_binary(&query_has_layout_hash(deps, layout_hash).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
        QueryMsg::GetTrackLayoutHash { track_id } => to_json_binary(&query_get_track_layout_hash(deps, track_id).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?),
    }
}

pub fn query_get_track(deps: Deps, track_id: Uint128) -> Result<Track, TrackManagerError> {
    // Always return the stored full track with precomputed progress
    let track = get_track(deps.storage, &track_id.into())?;
    Ok(track)
}

pub fn query_list_tracks(deps: Deps, start_after: Option<u128>, limit: Option<u32>) -> Result<crate::msg::ListTracksResponse, TrackManagerError> {
    let mut tracks = vec![];
    let start_after = if let Some(start_after) = start_after {
        Some(Bound::exclusive(start_after))
    } else {
        None
    };
    let limit = limit.unwrap_or(MAX_LIMIT);

    for item in TRACKS
        .range(deps.storage, start_after, None, Order::Ascending)
        .take(limit as usize) {
        let (_, track) = item?;
        tracks.push(track);
    }
    Ok(crate::msg::ListTracksResponse { tracks })
}

pub fn query_list_pvp_track_ids(deps: Deps, start_after: Option<u128>, limit: Option<u32>) -> Result<membrane::track_manager::PvpTrackIdsResponse, TrackManagerError> {
    let mut ids = vec![];
    let start_after = if let Some(sa) = start_after { Some(Bound::exclusive(sa)) } else { None };
    let limit = limit.unwrap_or(MAX_LIMIT).min(1024);
    for item in PVP_TRACK_IDS.range(deps.storage, start_after, None, Order::Ascending).take(limit as usize) {
        let (id, _) = item?;
        ids.push(id);
    }
    Ok(membrane::track_manager::PvpTrackIdsResponse { ids })
}

pub fn query_has_layout_hash(deps: Deps, layout_hash: String) -> Result<bool, TrackManagerError> {
    has_track_hash(deps.storage, &layout_hash)
}

pub fn query_get_track_layout_hash(deps: Deps, track_id: Uint128) -> Result<String, TrackManagerError> {
    get_track_layout_hash(deps.storage, &track_id.u128())
}

pub fn execute_edit_track(
    deps: DepsMut,
    info: MessageInfo,
    track_id: Uint128,
    name: Option<String>,
    delete: Option<bool>,
) -> Result<Response, TrackManagerError> {
    // Validate that at least one operation is specified
    if name.is_none() && delete.is_none() {
        return Err(TrackManagerError::InvalidEditOperation {});
    }

    // Get the existing track
    let mut track = get_track(deps.storage, &track_id.u128())?;

    // Check if the caller is the track creator (for now, we'll allow only the creator to edit)
    // In the future, this could be expanded to allow admin or other authorized users
    if info.sender.to_string() != track.creator {
        return Err(TrackManagerError::Unauthorized {});
    }

    // Handle deletion
    if let Some(should_delete) = delete {
        if should_delete {
            return execute_delete_track(deps, track_id, track);
        }
    }

    // Handle name update
    if let Some(new_name) = name {
        // Check for duplicate name using hash
        let new_name_hash = calculate_name_hash(&new_name);
        if has_track_name_hash(deps.storage, &new_name_hash)? {
            return Err(TrackManagerError::DuplicateTrackName {});
        }

        // Remove old name hash mapping
        if let Ok(old_name_hash) = get_track_name_hash(deps.storage, &track_id.u128()) {
            TRACK_NAME_HASHES.remove(deps.storage, old_name_hash);
        }

        // Update track name
        track.name = new_name.clone();
        set_track(deps.storage, &track_id.u128(), track)?;

        // Save new name hash
        save_track_name_hash(deps.storage, &new_name_hash, &track_id.u128())?;
        save_track_id_name_hash_mapping(deps.storage, &track_id.u128(), &new_name_hash)?;

        Ok(Response::new()
            .add_attribute("method", "edit_track")
            .add_attribute("track_id", track_id.to_string())
            .add_attribute("action", "name_update")
            .add_attribute("new_name", new_name))
    } else {
        // This shouldn't happen due to validation above, but just in case
        Err(TrackManagerError::InvalidEditOperation {})
    }
}

fn execute_delete_track(
    deps: DepsMut,
    track_id: Uint128,
    track: Track,
) -> Result<Response, TrackManagerError> {
    // TODO: Add checks for track usage (e.g., active races, tournaments, etc.)
    // For now, we'll allow deletion but this should be enhanced in the future
    
    // Remove the track from storage
    TRACKS.remove(deps.storage, track_id.u128());
    
    // Clean up hash mappings
    if let Ok(layout_hash) = get_track_layout_hash(deps.storage, &track_id.u128()) {
        TRACK_LAYOUT_HASHES.remove(deps.storage, layout_hash);
        TRACK_ID_TO_HASH.remove(deps.storage, track_id.u128());
    }
    
    // Clean up name hash mappings
    if let Ok(name_hash) = get_track_name_hash(deps.storage, &track_id.u128()) {
        TRACK_NAME_HASHES.remove(deps.storage, name_hash);
        TRACK_ID_TO_NAME_HASH.remove(deps.storage, track_id.u128());
    }
    
    // Remove from PvP tracks if it was there
    PVP_TRACK_IDS.remove(deps.storage, track_id.u128());

    Ok(Response::new()
        .add_attribute("method", "edit_track")
        .add_attribute("track_id", track_id.to_string())
        .add_attribute("action", "delete")
        .add_attribute("track_name", track.name))
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, TrackManagerError> {
    // let mut migrated_count = 0u32;
    // let mut error_count = 0u32;

    // // Collect all existing tracks first to avoid borrow checker issues
    // let tracks: Vec<(u128, Track)> = TRACKS
    //     .range(deps.storage, None, None, Order::Ascending)
    //     .collect::<Result<Vec<_>, _>>()
    //     .map_err(|_| TrackManagerError::StorageError {})?;

    // // Iterate through all existing tracks and populate name hashes
    // for (track_id, track) in tracks {
    //     // Calculate name hash for this track
    //     let name_hash = calculate_name_hash(&track.name);
        
    //     // Check if name hash already exists (shouldn't happen for existing tracks, but be safe)
    //     if !has_track_name_hash(deps.storage, &name_hash)? {
    //         // Save the name hash mapping
    //         save_track_name_hash(deps.storage, &name_hash, &track_id)?;
    //         save_track_id_name_hash_mapping(deps.storage, &track_id, &name_hash)?;
    //         migrated_count += 1;
    //     } else {
    //         // If name hash already exists, it means there's a duplicate name
    //         // We'll log this but continue processing other tracks
    //         error_count += 1;
    //     }
    // }

    Ok(Response::new()
        .add_attribute("method", "migrate")
        // .add_attribute("migrated_tracks", migrated_count.to_string())
        // .add_attribute("error_count", error_count.to_string())
    )
}

fn execute_recompute_progress(deps: DepsMut, track_id: Option<Uint128>) -> Result<Response, TrackManagerError> {
    // If a specific track_id is provided, recompute that one; otherwise recompute all
    let mut updated_count: u32 = 0;
    if let Some(id) = track_id {
        if let Ok(old) = get_track(deps.storage, &id.u128()) {
            let height = old.height as usize;
            let width = old.width as usize;
            let mut layout_props: Vec<Vec<TileProperties>> = vec![vec![TileProperties::default(); width]; height];
            for y in 0..height { for x in 0..width { layout_props[y][x] = old.layout[y][x].properties.clone(); } }
            let (track_layout, fastest_tick_time, starting_tiles) = calculate_progress_towards_finish(
                &layout_props, old.width, old.height
            );
            let new_track = Track { creator: old.creator, id: old.id, name: old.name, width: old.width, height: old.height, layout: track_layout, fastest_tick_time, starting_tiles };
            set_track(deps.storage, &id.u128(), new_track)?;
            updated_count += 1;
        }
    } else {
        // Recompute for all stored tracks (TRACKS map)
        let mut ids: Vec<u128> = vec![];
        for item in TRACKS.range(deps.storage, None, None, Order::Ascending) { ids.push(item?.0); }
        for id in ids {
            if let Ok(old) = get_track(deps.storage, &id) {
                let height = old.height as usize;
                let width = old.width as usize;
                let mut layout_props: Vec<Vec<TileProperties>> = vec![vec![TileProperties::default(); width]; height];
                for y in 0..height { for x in 0..width { layout_props[y][x] = old.layout[y][x].properties.clone(); } }
                let (track_layout, fastest_tick_time, starting_tiles) = calculate_progress_towards_finish(
                    &layout_props, old.width, old.height
                );
                let new_track = Track { creator: old.creator, id: old.id, name: old.name, width: old.width, height: old.height, layout: track_layout, fastest_tick_time, starting_tiles };
                set_track(deps.storage, &id, new_track)?;
                updated_count += 1;
            }
        }
    }
    Ok(Response::new().add_attribute("action", "recompute_progress").add_attribute("updated", updated_count.to_string()))
}
