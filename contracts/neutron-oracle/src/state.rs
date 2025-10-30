use cw_storage_plus::{Item, Map};
use cosmwasm_std::Addr;
use membrane::neutron_oracle::Config;
use membrane::types::NeutronOracleInfo;


pub const CONFIG: Item<Config> = Item::new("config");
pub const ASSETS: Map<String, NeutronOracleInfo> = Map::new("assets"); //Asset, Vec of Oracles for each basket

pub const OWNERSHIP_TRANSFER: Item<Addr> = Item::new("ownership_transfer");
