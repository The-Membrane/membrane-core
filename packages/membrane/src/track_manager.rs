use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::types::{Track, TrackTile, TileProperties};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    AddTrack {
        name: String,
        width: u8,
        height: u8,
        layout: Vec<Vec<TileProperties>>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Track)]
    GetTrack { track_id: Uint128 },
    #[returns(ListTracksResponse)]
    ListTracks {
        start_after: Option<u128>,
        limit: Option<u32>,
    },
}

// #[cw_serde]
// pub struct GetTrackResponse {
//     pub track: Track,
// }

#[cw_serde]
pub struct ListTracksResponse {
    pub tracks: Vec<Track>,
} 