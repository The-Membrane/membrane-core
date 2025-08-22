

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
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    Base(Cw721QueryMsg),
}


// Accepted payment options for mint and owner
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub payment_options: Vec<Coin>,
    pub race_engine_contract: Option<String>,
}

#[cw_serde]
pub struct MigrateMsg {}
