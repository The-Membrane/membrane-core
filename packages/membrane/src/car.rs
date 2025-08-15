

// car_nft/src/msg.rs

use cosmwasm_schema::cw_serde;
use cosmwasm_schema::QueryResponses;
use cosmwasm_std::Addr;

use crate::types::CarMetadata;
use cosmwasm_std::Coin;

#[cw_serde]
pub struct InstantiateMsg {
    pub name: String,
    pub symbol: String,
    pub payment_options: Option<Vec<Coin>>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Forward all standard CW721 executes through this variant
    Base(cw721_base::ExecuteMsg<Option<CarMetadata>, cosmwasm_std::Empty>),
    /// Request the contract to mint a new NFT. The contract will mint by self-calling,
    /// so only the contract (minter) can actually perform the mint.
    MintCar {
        owner: String,
        token_uri: Option<String>,
        extension: Option<CarMetadata>,
    },
    /// Update configuration and optionally begin/complete two-step owner transfer
    UpdateConfig {
        payment_options: Option<Vec<Coin>>,
        new_owner: Option<String>,
    },
    /// Owner-only: update the custom decal SVG for a token
    UpdateCustomDecal {
        token_id: String,
        svg: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    Base(cw721_base::QueryMsg<cosmwasm_std::Empty>),
}


// Accepted payment options for mint and owner
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub payment_options: Vec<Coin>,
}
