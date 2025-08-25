use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};

use membrane::types::CarMetadata;
use membrane::car::Config;


// Pending owner transfer
pub const PENDING_OWNER: Item<Addr> = Item::new("pending_owner");

pub const CONFIG: Item<Config> = Item::new("config");

// Car information: car_id -> CarInfo
pub const CAR_INFO: Map<u128, CarInfo> = Map::new("car_info");

// Car ID counter
pub const CAR_ID_COUNTER: Item<Uint128> = Item::new("car_id_counter");

// Q-table storage: (car_id, state_hash) -> [i32; 4]
pub const Q_TABLE: Map<(u128, &str), [i32; 4]> = Map::new("q_table");

// Used trait combinations encoded as compact u64 bit patterns
pub const USED_TRAIT_COMBOS: Map<u64, bool> = Map::new("used_trait_combos");

// Registry to ensure car name uniqueness: hashed name key -> true
pub const NAME_REGISTRY: Map<u128, bool> = Map::new("car_names");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PendingFreeCar {
	pub reserved_for: Addr,
	pub expires_at_nanos: u64,
	pub trait_code: u64,
}

// Map of pending free cars by car_id
pub const PENDING_FREE_CARS: Map<u128, PendingFreeCar> = Map::new("pending_free_cars");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CarInfo {
	pub owners: Vec<Addr>,
	pub metadata: Option<CarMetadata>,
	pub created_at: u64,
	// Energy system
	pub current_energy: u32,
	pub last_energy_update_nanos: u64,
}

impl CarInfo {
	pub fn recover_energy(&mut self, now_nanos: u64, cfg: &Config) {
		let max_energy = cfg.max_energy as u64;
		if self.current_energy as u64 >= max_energy { self.last_energy_update_nanos = now_nanos; return; }
		let elapsed = now_nanos.saturating_sub(self.last_energy_update_nanos);
		if cfg.energy_recovery_hours == 0 { return; }
		let full_recover_ns: u64 = (cfg.energy_recovery_hours as u64)
			.saturating_mul(60).saturating_mul(60).saturating_mul(1_000_000_000);
		if full_recover_ns == 0 { return; }
		let recovered = ((elapsed as u128)
			.saturating_mul(cfg.max_energy as u128)
			/ (full_recover_ns as u128)) as u32;
		if recovered > 0 {
			let new_energy = (self.current_energy as u64 + recovered as u64).min(max_energy) as u32;
			self.current_energy = new_energy;
			// advance last update proportionally to consumed elapsed that produced recovery
			let used = (recovered as u128)
				.saturating_mul(full_recover_ns as u128)
				/ (cfg.max_energy as u128);
			self.last_energy_update_nanos = self.last_energy_update_nanos.saturating_add(used as u64);
		}
	}
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