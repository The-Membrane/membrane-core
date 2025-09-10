// Note: rps_engine module is commented out in membrane package
// Define messages locally for now
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;
use crate::race_engine::TrainingConfig;
use crate::types::{RpsRewardConfig, SeriesMode, TickRecord};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub car_contract: String,
    pub max_ticks: Option<u32>,
    pub match_history_limit: Option<u32>,
    pub tick_history_limit: Option<u32>,
}

#[cw_serde]
pub enum ExecuteMsg {
    PlaySeries { car_id: u128, opponent_id: Option<u128>, train: bool, training_config: Option<TrainingConfig>, reward_config: Option<RpsRewardConfig>, mode: SeriesMode },
    UpdateConfig { max_ticks: Option<u32>, match_history_limit: Option<u32>, tick_history_limit: Option<u32> },
    PurgeCar { car_id: u128 },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    GetConfig {},
    #[returns(GetQResponse)]
    GetQ { car_id: u128, state_id: Option<u8> },
    #[returns(GetHistoryResponse)]
    GetHistory { car_id: u128 },
    #[returns(GetTickHistoryResponse)]
    GetTickHistory { car_id: u128 },
}

#[cw_serde]
pub struct GetQResponseEntry { pub state_id: u8, pub action_values: [i8; 3] }

#[cw_serde]
pub struct GetQResponse { pub car_id: u128, pub q_values: Vec<GetQResponseEntry> }

#[cw_serde]
pub struct GetHistoryResponse { pub car_id: u128, pub history: Vec<u8> }

#[cw_serde]
pub struct GetTickHistoryResponse { pub car_id: u128, pub ticks: Vec<TickRecord> }

#[cw_serde]
pub struct ConfigResponse {
    pub admin: String,
    pub car_contract: String,
    pub max_ticks: u32,
    pub match_history_limit: u32,
    pub tick_history_limit: u32,
}
