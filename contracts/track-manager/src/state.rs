use cosmwasm_std::{Uint128, StdResult, Storage};
use cw_storage_plus::{Item, Map, SnapshotMap, Strategy};
use membrane::types::Track;

pub const ADMIN: Item<cosmwasm_std::Addr> = Item::new("admin");
pub const TRACKS: Map<u128, Track> = Map::new("tracks");
pub const TRACK_ID_COUNTER: Item<Uint128> = Item::new("track_id_counter");

// New: PvP track ids set
pub const PVP_TRACK_IDS: Map<u128, bool> = Map::new("pvp_track_ids");

pub fn get_track(storage: &dyn Storage, track_id: &u128) -> Result<Track, crate::error::TrackManagerError> {
    TRACKS.load(storage, *track_id).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

pub fn set_track(storage: &mut dyn Storage, track_id: &u128, track: Track) -> Result<(), crate::error::TrackManagerError> {
    TRACKS.save(storage, *track_id, &track).map_err(|_| crate::error::TrackManagerError::TrackNotFound { track_id: (*track_id).to_string() })
}

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
