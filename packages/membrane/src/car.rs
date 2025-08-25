

// car_nft/src/msg.rs

use cosmwasm_schema::cw_serde;
use cosmwasm_schema::QueryResponses;
use cosmwasm_std::Addr;

use crate::types::CarMetadata;
use cosmwasm_std::Coin;

// Type alias to avoid generic parameter issues in enum variants
pub type Cw721ExecuteMsg = cw721_base::ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty>;
pub type Cw721QueryMsg = cw721_base::QueryMsg<cosmwasm_std::Empty>;

// Add a shared maximum name size for validation in contracts/clients
pub const MAX_NAME_SIZE: usize = 240;

#[cw_serde]
pub struct InstantiateMsg {
    pub name: String,
    pub symbol: String,
    pub payment_options: Option<Vec<Coin>>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Forward all standard CW721 executes through this variant
    Base(Cw721ExecuteMsg),
    /// Request the contract to mint a new NFT. The contract will mint by self-calling,
    /// so only the contract (minter) can actually perform the mint.
    CreateCar {
        owner: Option<String>,
        token_uri: Option<String>,
        extension: Option<CarMetadata>,
    },
    /// Update configuration and optionally begin/complete two-step owner transfer
    UpdateConfig {
        payment_options: Option<Vec<Coin>>,
        new_owner: Option<String>,
        race_engine_contract: Option<String>,
    },
    /// Owner-only: update energy parameters
    UpdateEnergyParams {
        max_energy: Option<u32>,
        energy_recovery_hours: Option<u32>,
        energy_per_training: Option<u32>,
    },
    /// Owner-only: update training payment options
    UpdateTrainingPayments {
        training_payment_options: Vec<Coin>,
    },
    /// Owner-only: update the custom decal SVG for a token
    UpdateCustomDecal {
        token_id: String,
        svg: String,
    },
    /// Owner-only: update the car name, with length and uniqueness checks
    UpdateCarName {
        token_id: String,
        new_name: String,
    },
    /// Pay for a free-minted car before it expires to finalize ownership (removes time limit)
    PayToFinalize {
        token_id: String,
    },
    /// Expire and delete a pending free car if its time has elapsed
    ExpireCar {
        token_id: String,
    },
    /// Pay to refill a car's training energy to full using configured training payment options
    PayForTraining {
        token_id: String,
    },
    /// Race engine only: consume energy for one or more training sessions
    ConsumeTrainingEnergy {
        token_id: String,
        sessions: u32,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    Base(Cw721QueryMsg),
    #[returns(CarInfoResponse)]
    GetCarInfo { token_id: String },
}


// Accepted payment options for mint and owner
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub payment_options: Vec<Coin>,
    pub race_engine_contract: Option<String>,
    // Energy system configuration
    pub max_energy: u32,
    /// Hours required to fully recover from 0 to max energy (linear regen)
    pub energy_recovery_hours: u32,
    /// Energy units consumed per training session
    pub energy_per_training: u32,
    /// Accepted payment options for refilling training energy
    pub training_payment_options: Vec<Coin>,
}

#[cw_serde]
pub struct MigrateMsg {}

// Query responses
#[cw_serde]
pub struct CarInfoResponse {
    pub owners: Vec<String>,
    pub metadata: Option<CarMetadata>,
    pub created_at: u64,
    pub current_energy: u32,
    pub last_energy_update_nanos: u64,
    // Echo selected config values to support UI without another query
    pub max_energy: u32,
    pub energy_recovery_hours: u32,
    pub energy_per_training: u32,
    pub training_payment_options: Vec<Coin>,
}
