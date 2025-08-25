use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Uint128, CosmosMsg};

#[cw_serde]
pub enum EventType { Maze, Pvp }

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub track_manager_contract: String,
    pub race_engine_contract: String,
    pub car_contract: String,
    pub subdenom: String,
    /// Only add one for testing, rev contract will use TFPassthrough
    pub tokenfactory_contract: Option<String>,
    pub mint_amount: Uint128,
    pub maze_default_difficulty: Option<u8>,
    pub maze_width: Option<u8>,
    pub maze_height: Option<u8>,
    pub maze_event_cadence_seconds: u64,
    pub maze_event_window_seconds: u64,
    pub pvp_event_cadence_seconds: u64,
    pub pvp_event_window_seconds: u64,
    pub min_progress_to_finish_per_start_tile: Option<u16>,
    pub min_start_tile_progress_threshold: Option<u16>,
    pub max_start_tile_progress_diff: Option<u16>,
    pub revenue_contract: Option<String>,
    /// Keep this false for testing 
    pub create_denom: Option<bool>,
}

#[cw_serde]
pub enum ExecuteMsg {
    GenerateMaze { name: String },
    StartNewWindows {},
    SetEventConfig {
        maze_cadence_seconds: Option<u64>,
        maze_window_seconds: Option<u64>,
        pvp_cadence_seconds: Option<u64>,
        pvp_window_seconds: Option<u64>,
    },
    RecordWin { event: EventType, car_id: u128 },
    TokenfactoryPassthrough { msgs: Vec<CosmosMsg> },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(VerifyEventRaceResponse)]
    VerifyEventRace { track_id: u128, car_ids: Vec<u128>, pvp: bool },
    #[returns(Config)]
    GetConfig {},
    #[returns(u64)]
    SecondsUntilOpen { event: EventType },
}

#[cw_serde]
pub struct VerifyEventRaceResponse {
    pub allowed: bool,
    pub event: Option<EventType>,
    pub required_opponent: Option<u128>,
}

#[cw_serde]
pub struct ConfigResponse { pub config: Config }

#[cw_serde]
pub struct Config {
    pub admin: String,
    pub track_manager_contract: String,
    pub race_engine_contract: String,
    pub car_contract: String,
    pub tokenfactory_denom: String,
    pub tokenfactory_contract: Option<String>,
    pub mint_amount: Uint128,
    pub maze_default_difficulty: u8,
    pub maze_width: u8,
    pub maze_height: u8,
    pub maze_event_cadence_seconds: u64,
    pub maze_event_window_seconds: u64,
    pub pvp_event_cadence_seconds: u64,
    pub pvp_event_window_seconds: u64,
    pub min_progress_to_finish_per_start_tile: u16,
    pub min_start_tile_progress_threshold: u16,
    pub max_start_tile_progress_diff: u16,
    pub revenue_contract: Option<String>,
}

#[cw_serde]
pub struct AmountPerEvent {
    pub event: EventType,
    pub window_start: u64,
    pub amount_minted: Uint128,
}

#[cw_serde]
pub struct MigrateMsg {} 