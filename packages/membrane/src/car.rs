

// car_nft/src/msg.rs

use cosmwasm_schema::cw_serde;
use cosmwasm_schema::QueryResponses;
use cosmwasm_std::Addr;
use cosmwasm_std::Binary;
use cw721_base::{MintMsg};
use cw721::Expiration;
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
pub enum ExecuteMsg<T, E> {
    /////////////cw721 base messages //////////////
    /// Transfer is a base message to move a token to another account without triggering actions
    TransferNft { recipient: String, token_id: String },
    /// Send is a base message to transfer a token to a contract and trigger an action
    /// on the receiving contract.
    SendNft {
        contract: String,
        token_id: String,
        msg: Binary,
    },
    /// Allows operator to transfer / send the token from the owner's account.
    /// If expiration is set, then this allowance has a time/height limit
    Approve {
        spender: String,
        token_id: String,
        expires: Option<Expiration>,
    },
    /// Remove previously granted Approval
    Revoke { spender: String, token_id: String },
    /// Allows operator to transfer / send any token from the owner's account.
    /// If expiration is set, then this allowance has a time/height limit
    ApproveAll {
        operator: String,
        expires: Option<Expiration>,
    },
    /// Remove previously granted ApproveAll permission
    RevokeAll { operator: String },

    /// Mint a new NFT, can only be called by the contract minter
    Mint(MintMsg<T>),

    /// Burn an NFT the sender has access to
    Burn { token_id: String },

    /// Extension msg
    Extension { msg: E },
    ///////////////////////////
    /// Request the contract to mint a new NFT. The contract will mint by self-calling,
    /// so only the contract (minter) can actually perform the mint.
    CreateCar {
        name: String,
        owner: Option<String>,
        token_uri: Option<String>,
    },
    /// Update configuration and optionally begin/complete two-step owner transfer
    UpdateConfig {
        payment_options: Option<Vec<Coin>>,
        new_owner: Option<String>,
        race_engine_contract: Option<String>,
        revenue_contract: Option<String>,
        energy_consumers: Option<crate::types::StringEntry>,
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
pub enum QueryMsg {
    //CW721QueryMsgs
    /// Return the owner of the given token, error if token does not exist
    /// Return type: OwnerOfResponse
    OwnerOf {
        token_id: String,
        /// unset or false will filter out expired approvals, you must set to true to see them
        include_expired: Option<bool>,
    },
    /// Return operator that can access all of the owner's tokens.
    /// Return type: `ApprovalResponse`
    Approval {
        token_id: String,
        spender: String,
        include_expired: Option<bool>,
    },
    /// Return approvals that a token has
    /// Return type: `ApprovalsResponse`
    Approvals {
        token_id: String,
        include_expired: Option<bool>,
    },
    /// List all operators that can access all of the owner's tokens
    /// Return type: `OperatorsResponse`
    AllOperators {
        owner: String,
        /// unset or false will filter out expired items, you must set to true to see them
        include_expired: Option<bool>,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Total number of tokens issued
    NumTokens {},

    /// With MetaData Extension.
    /// Returns top-level metadata about the contract: `ContractInfoResponse`
    ContractInfo {},
    /// With MetaData Extension.
    /// Returns metadata about one particular token, based on *ERC721 Metadata JSON Schema*
    /// but directly from the contract: `NftInfoResponse`
    NftInfo {
        token_id: String,
    },
    /// With MetaData Extension.
    /// Returns the result of both `NftInfo` and `OwnerOf` as one query as an optimization
    /// for clients: `AllNftInfo`
    AllNftInfo {
        token_id: String,
        /// unset or false will filter out expired approvals, you must set to true to see them
        include_expired: Option<bool>,
    },

    /// With Enumerable extension.
    /// Returns all tokens owned by the given address, [] if unset.
    /// Return type: TokensResponse.
    Tokens {
        owner: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// With Enumerable extension.
    /// Requires pagination. Lists all token_ids controlled by the contract.
    /// Return type: TokensResponse.
    AllTokens {
        start_after: Option<String>,
        limit: Option<u32>,
    },

    // Return the minter
    Minter {},
    ///////////////
    // #[returns(CarInfoResponse)]
    GetCarInfo { token_id: String },
}


// Accepted payment options for mint and owner
#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub payment_options: Vec<Coin>,
    pub race_engine_contract: Option<String>,
    pub revenue_contract: Option<String>,
    // Energy system configuration
    pub max_energy: u32,
    /// Hours required to fully recover from 0 to max energy (linear regen)
    pub energy_recovery_hours: u32,
    /// Energy units consumed per training session
    pub energy_per_training: u32,
    /// Accepted payment options for refilling training energy
    pub training_payment_options: Vec<Coin>,
    /// Valid contracts that can consume energy
    pub valid_energy_consumers: Vec<String>,
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
