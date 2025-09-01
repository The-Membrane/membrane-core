use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{CosmosMsg, Decimal, Uint128};

#[cw_serde]
pub enum EventType {
    Maze,
    Pvp,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub track_manager_contract: String,
    pub race_engine_contract: String,
    pub car_contract: String,
    pub subdenom: String,
    pub tokenfactory_contract: Option<String>,
    pub mint_amount: Uint128,
    pub runner_reward_rate: Option<Decimal>,
    pub maze_default_difficulty: Option<u8>,
    pub maze_width: Option<u8>,
    pub maze_height: Option<u8>,
    pub maze_event_cadence_seconds: u64,
    pub maze_event_window_seconds: u64,
    pub pvp_event_cadence_seconds: u64,
    pub pvp_event_window_seconds: u64,
    /// Enable or disable PvP features. Defaults to true when omitted.
    pub pvp_enabled: Option<bool>,
    pub min_start_tile_progress_threshold: Option<u16>,
    pub max_start_tile_progress_diff: Option<u16>,
    pub revenue_contract: Option<String>,
    /// Keep this false for testing 
    pub create_denom: Option<bool>,
    /// Difficulty adjustment configuration
    pub difficulty_adjustment_config: Option<DifficultyAdjustmentConfig>,
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
        pvp_enabled: Option<bool>,
        runner_reward_rate: Option<Decimal>,
    },
    SetDifficultyAdjustmentConfig { config: DifficultyAdjustmentConfig },
    RecordWin { event: EventType, car_id: u128, runner: String },
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
    #[returns(Option<u128>)]
    ValidMazeID { },
    #[returns(Vec<u128>)]
    GetRecordedWins { 
        event: EventType,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(WindowStatusResponse)]
    GetWindowStatus { event: EventType },
    #[returns(DifficultyAdjustmentInfo)]
    GetDifficultyAdjustmentInfo { event: EventType },
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
    pub runner_reward_rate: Decimal,
    pub maze_default_difficulty: u8,
    pub maze_width: u8,
    pub maze_height: u8,
    pub maze_event_cadence_seconds: u64,
    pub maze_event_window_seconds: u64,
    pub pvp_event_cadence_seconds: u64,
    pub pvp_event_window_seconds: u64,
    pub pvp_enabled: bool,
    pub min_start_tile_progress_threshold: u16,
    pub max_start_tile_progress_diff: u16,
    pub revenue_contract: Option<String>,
}

#[cw_serde]
pub struct DifficultyAdjustmentConfig {
    pub enabled: bool,
    pub history_window_size: u32, // Number of windows to keep in history
    pub difficulty_increase_threshold: f64, // Multiplier above historical average to trigger difficulty increase
    pub difficulty_decrease_threshold: f64, // Multiplier below historical average to trigger difficulty decrease
    pub max_difficulty: u8, // Maximum difficulty level
    pub min_difficulty: u8, // Minimum difficulty level
    pub difficulty_step: u8, // How much to increase/decrease difficulty by
    pub min_history_for_adjustment: u32, // Minimum windows needed before making adjustments
}

impl Default for DifficultyAdjustmentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_window_size: 10, // Keep last 10 windows
            difficulty_increase_threshold: 1.3, // Increase difficulty if 1.3x above historical average
            difficulty_decrease_threshold: 0.7, // Decrease difficulty if 0.7x below historical average
            max_difficulty: 10, // Maximum difficulty level
            min_difficulty: 1, // Minimum difficulty level
            difficulty_step: 1, // Increase/decrease by 1
            min_history_for_adjustment: 3, // Need at least 3 windows before making adjustments
        }
    }
}

#[cw_serde]
pub struct DifficultyAdjustmentInfo {
    pub current_difficulty: u8,
    pub current_win_count: u32,
    pub historical_average: f64,
    pub difficulty_adjustment: Option<DifficultyAdjustment>,
    pub config: DifficultyAdjustmentConfig,
    pub windows_in_history: u32,
}

#[cw_serde]
pub struct DifficultyAdjustment {
    pub old_difficulty: u8,
    pub new_difficulty: u8,
    pub reason: String,
    pub current_win_count: u32,
    pub historical_average: f64,
    pub adjustment_threshold: f64,
}

#[cw_serde]
pub struct AmountPerEvent {
    pub event: EventType,
    pub window_start: u64,
    pub amount_minted: Uint128,
}

#[cw_serde]
pub struct MigrateMsg {} 

#[cw_serde]
pub struct WindowStatusResponse {
    pub is_active: bool,
    pub window_start: u64,
    pub window_end: u64,
    pub seconds_until_open: u64,
    pub seconds_until_close: u64,
} 