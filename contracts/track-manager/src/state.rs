use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};

use racing::types::{Track, TrackTile};

pub const ADMIN: Item<Addr> = Item::new("admin");
pub const TRACK_ID_COUNTER: Item<Uint128> = Item::new("track_id_counter");

// Track storage: track_id -> Track
pub const TRACKS: Map<u128, Track> = Map::new("tracks");


pub fn get_track(storage: &dyn Storage, track_id: &u128) -> StdResult<Track> {
    TRACKS.load(storage, *track_id)
}

pub fn set_track(storage: &mut dyn Storage, track_id: &u128, track: Track) -> StdResult<()> {
    TRACKS.save(storage, *track_id, &track)
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
