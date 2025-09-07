use cosmwasm_std::{Uint128, Storage};
use cw_storage_plus::{Item, Map};
use membrane::types::Track;

pub const ADMIN: Item<cosmwasm_std::Addr> = Item::new("admin");
pub const TRACKS: Map<u128, Track> = Map::new("tracks");
pub const TRACK_ID_COUNTER: Item<Uint128> = Item::new("track_id_counter");

// New: PvP track ids set
pub const PVP_TRACK_IDS: Map<u128, bool> = Map::new("pvp_track_ids");

// New: Track layout hashes for duplicate detection
pub const TRACK_LAYOUT_HASHES: Map<String, u128> = Map::new("track_layout_hashes");

// New: Reverse mapping from track ID to layout hash
pub const TRACK_ID_TO_HASH: Map<u128, String> = Map::new("track_id_to_hash");

// New: Track name hashes for duplicate detection
pub const TRACK_NAME_HASHES: Map<String, u128> = Map::new("track_name_hashes");

// New: Reverse mapping from track ID to name hash
pub const TRACK_ID_TO_NAME_HASH: Map<u128, String> = Map::new("track_id_to_name_hash");

pub fn get_track(storage: &dyn Storage, track_id: &u128) -> Result<Track, crate::error::TrackManagerError> {
    TRACKS.load(storage, *track_id).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

pub fn set_track(storage: &mut dyn Storage, track_id: &u128, track: Track) -> Result<(), crate::error::TrackManagerError> {
    TRACKS.save(storage, *track_id, &track).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

// New: Helper functions for hash management
pub fn save_track_hash(storage: &mut dyn Storage, layout_hash: &str, track_id: &u128) -> Result<(), crate::error::TrackManagerError> {
    TRACK_LAYOUT_HASHES.save(storage, layout_hash.to_string(), track_id).map_err(|_| crate::error::TrackManagerError::StorageError {})
}

pub fn has_track_hash(storage: &dyn Storage, layout_hash: &str) -> Result<bool, crate::error::TrackManagerError> {
    Ok(TRACK_LAYOUT_HASHES.has(storage, layout_hash.to_string()))
}

// New: Helper functions for reverse mapping
pub fn save_track_id_hash_mapping(storage: &mut dyn Storage, track_id: &u128, layout_hash: &str) -> Result<(), crate::error::TrackManagerError> {
    TRACK_ID_TO_HASH.save(storage, *track_id, &layout_hash.to_string()).map_err(|_| crate::error::TrackManagerError::StorageError {})
}

pub fn get_track_layout_hash(storage: &dyn Storage, track_id: &u128) -> Result<String, crate::error::TrackManagerError> {
    TRACK_ID_TO_HASH.load(storage, *track_id).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

// New: Helper functions for name hash management
pub fn save_track_name_hash(storage: &mut dyn Storage, name_hash: &str, track_id: &u128) -> Result<(), crate::error::TrackManagerError> {
    TRACK_NAME_HASHES.save(storage, name_hash.to_string(), track_id).map_err(|_| crate::error::TrackManagerError::StorageError {})
}

pub fn has_track_name_hash(storage: &dyn Storage, name_hash: &str) -> Result<bool, crate::error::TrackManagerError> {
    Ok(TRACK_NAME_HASHES.has(storage, name_hash.to_string()))
}

pub fn save_track_id_name_hash_mapping(storage: &mut dyn Storage, track_id: &u128, name_hash: &str) -> Result<(), crate::error::TrackManagerError> {
    TRACK_ID_TO_NAME_HASH.save(storage, *track_id, &name_hash.to_string()).map_err(|_| crate::error::TrackManagerError::StorageError {})
}

pub fn get_track_name_hash(storage: &dyn Storage, track_id: &u128) -> Result<String, crate::error::TrackManagerError> {
    TRACK_ID_TO_NAME_HASH.load(storage, *track_id).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

// Removed compressed track storage; we store full tracks directly

// pub fn add_track_to_all_tracks(storage: &mut dyn Storage, track_id: &Uint128) -> StdResult<()> {
//     ALL_TRACKS.save(storage, track_id, &true)
// }

// pub fn get_all_tracks(storage: &dyn Storage) -> StdResult<Vec<Uint128>> {
//     let mut tracks = vec![];
//     let range = ALL_TRACKS.range(storage, None, None, cosmwasm_std::Order::Ascending);
//     for item in range {
//         let (track_id, _) = item?;
//         tracks.push(track_id);
//     }
//     Ok(tracks)
// }
