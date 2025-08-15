use cosmwasm_std::{Addr, Coin, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use racing::types::{CarMetadata, QTableEntry};
use racing::car::Config;


// Pending owner transfer
pub const PENDING_OWNER: Item<Addr> = Item::new("pending_owner");

pub const CONFIG: Item<Config> = Item::new("config");

// Car information: car_id -> CarInfo
pub const CAR_INFO: Map<u128, CarInfo> = Map::new("car_info");

// Car ID counter
pub const CAR_ID_COUNTER: Item<Uint128> = Item::new("car_id_counter");

// Q-table storage: (car_id, state_hash) -> [i32; 4]
pub const Q_TABLE: Map<(u128, &str), [i32; 4]> = Map::new("q_table");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CarInfo {
    pub owners: Vec<Addr>,
    pub metadata: Option<CarMetadata>,
    pub created_at: u64,
}

pub fn get_car_info(storage: &dyn Storage, car_id: u128) -> StdResult<CarInfo> {
    CAR_INFO.load(storage, car_id)
}

pub fn set_car_info(storage: &mut dyn Storage, car_id: u128, car_info: CarInfo) -> StdResult<()> {
    CAR_INFO.save(storage, car_id, &car_info)
}



// pub fn add_car_to_all_cars(storage: &mut dyn Storage, car_id: &Uint128) -> StdResult<()> {
//     ALL_CARS.save(storage, car_id.to_string().as_str(), &true)
// }

// pub fn remove_car_from_all_cars(storage: &mut dyn Storage, car_id: &Uint128) -> StdResult<()> {
//     ALL_CARS.remove(storage, car_id.to_string().as_str()    );
//     Ok(())
// }

// pub fn get_all_cars(storage: &dyn Storage) -> StdResult<Vec<String>> {
//     let mut cars = vec![];
//     let range = ALL_CARS.range(storage, None, None, cosmwasm_std::Order::Ascending);
//     for item in range {
//         let (car_id, _) = item?;
//         cars.push(car_id);
//     }
//     Ok(cars)
// } 