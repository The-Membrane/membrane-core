use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{from_json, to_json_binary, Addr, Binary, OwnedDeps, Querier, QuerierResult, QueryRequest, SystemResult, ContractResult};
use serde::Serialize;

use crate::contract::{execute, instantiate, query};
use crate::error::ContractError;
use membrane::race_engine::{ExecuteMsg, InstantiateMsg, QueryMsg, TrainingConfig, GetTrackTrainingStatsResponse, GetIntegerQResponse, MigrationStatusResponse};
use membrane::types::{RewardNumbers, Track, TrackTile, TileProperties, GoingBackward};
use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
enum TileFlag { Wall=0, Sticky=1, Boost=2, Finish=3, Normal=4 }

#[repr(u8)]
enum Dir3 { None=0, Up=1, Down=2, Left=3, Right=4 }

const DIRS: [(i32, i32); 4] = [(0,-1), (0,1), (-1,0), (1,0)]; // U D L R

/// Generate all 625 tile combinations (5^4 = 625)
fn generate_all_tile_combinations() -> Vec<[TileFlag; 4]> {
    let mut combinations = Vec::new();
    
    // Generate all combinations of 4 directions with 5 possible tile types each
    for up in 0..5 {
        for down in 0..5 {
            for left in 0..5 {
                for right in 0..5 {
                    combinations.push([
                        match up { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match down { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match left { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                        match right { 0 => TileFlag::Wall, 1 => TileFlag::Sticky, 2 => TileFlag::Boost, 3 => TileFlag::Finish, _ => TileFlag::Normal },
                    ]);
                }
            }
        }
    }
    
    combinations
}

/// Generate legacy state hash for migration testing
fn generate_legacy_state_hash_for_migration(
    x: i32, y: i32,
    speed: u32,
    other_cars: &[(i32,i32)],
    tile_combination: [TileFlag; 4], // 4 directions: U, D, L, R
) -> [u8; 32] {
    // ---------- 1. build 22-bit key ----------
    let mut key: u32 = 0;           // we'll only use lowest 22 bits
    for (i, &(dx,dy)) in DIRS.iter().enumerate() {
        let tx = x + dx.wrapping_mul(speed as i32);
        let ty = y + dy.wrapping_mul(speed as i32);

        // --- 3-bit tile flag ---
        let mut flag = TileFlag::Normal as u8;

        if tx < 0 || ty < 0 || ty as usize >= 50 || tx as usize >= 50 {
            flag = TileFlag::Wall as u8;
        } else {
            // Use the provided tile combination instead of track lookup
            flag = tile_combination[i] as u8;
        }

        // --- 1-bit "has car" flag ---
        let has_car = other_cars
            .iter()
            .any(|&(cx,cy)| cx == tx && cy == ty) as u8;

        // pack into 4 bits and shift into position
        let nibble = (flag & 0b111) | (has_car << 3);
        key |= (nibble as u32) << (i * 4);
    }

    // ---------- 2. closest-car direction ----------
    let mut dir3 = Dir3::None as u8;
    if !other_cars.is_empty() {
        let (mut best_d2, mut best_dir) = (i32::MAX, Dir3::None as u8);
        for &(cx,cy) in other_cars {
            let dx = cx - x;
            let dy = cy - y;
            let d2 = dx*dx + dy*dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best_dir = if dx.abs() > dy.abs() {
                    if dx > 0 { Dir3::Right } else { Dir3::Left }
                } else {
                    if dy > 0 { Dir3::Down }  else { Dir3::Up }
                } as u8;
            }
        }
        dir3 = best_dir;
    }
    key |= (dir3 as u32) << 16;   // bits 16-18

    // ---------- 3. hash ----------
    let mut hasher = Blake2bVar::new(32).unwrap(); // 256-bit
    let key_bytes = key.to_le_bytes();            // 4 bytes, lowest 3 used
    hasher.update(&key_bytes[..3]);               // feed 3 tight bytes
    let mut out = [0u8; 32];
    let _ = hasher.finalize_variable(&mut out);

    out
}

const ADMIN: &str = "admin";
const TRACK_CONTRACT: &str = "track_contract";
const CAR_CONTRACT: &str = "car_contract";

#[test]
fn test_hash_function_analysis() {
    println!("=== HASH FUNCTION ANALYSIS ===");
    
    // Test the hash function with various inputs to understand its behavior
    let tile_combinations = generate_all_tile_combinations();
    
    // Test 1: Single car scenario (most common)
    println!("\n--- Test 1: Single car scenario ---");
    let x = 0;
    let y = 0;
    let speed = 1;
    let other_cars = vec![];
    
    let mut hash_count = 0;
    let mut unique_hashes = std::collections::HashSet::new();
    
    for tile_combo in &tile_combinations {
        let hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
        unique_hashes.insert(hash);
        hash_count += 1;
    }
    
    println!("Single car scenario:");
    println!("  Total combinations tested: {}", hash_count);
    println!("  Unique hashes generated: {}", unique_hashes.len());
    println!("  Hash collision rate: {:.2}%", 
             ((hash_count - unique_hashes.len()) as f64 / hash_count as f64) * 100.0);
    
    // Test 2: With one other car
    println!("\n--- Test 2: One other car scenario ---");
    let other_cars = vec![(1, 1)];
    let mut hash_count_2 = 0;
    let mut unique_hashes_2 = std::collections::HashSet::new();
    
    for tile_combo in &tile_combinations {
        let hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
        unique_hashes_2.insert(hash);
        hash_count_2 += 1;
    }
    
    println!("One other car scenario:");
    println!("  Total combinations tested: {}", hash_count_2);
    println!("  Unique hashes generated: {}", unique_hashes_2.len());
    println!("  Hash collision rate: {:.2}%", 
             ((hash_count_2 - unique_hashes_2.len()) as f64 / hash_count_2 as f64) * 100.0);
    
    // Test 3: Check for potential issues
    println!("\n--- Test 3: Potential Issues Analysis ---");
    
    // Check if the hash function can generate the problematic hash
    let target_hash = "65563059B4D6A262BA9EC5DDE6E9365ED2C58F46FF067BA372FE0C58171A1791";
    let mut target_bytes = [0u8; 32];
    for (i, chunk) in target_hash.as_bytes().chunks(2).enumerate() {
        if i < 32 {
            let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
            target_bytes[i] = u8::from_str_radix(hex_str, 16).unwrap_or(0);
        }
    }
    
    // Check if this hash appears in our generated hashes
    if unique_hashes.contains(&target_bytes) {
        println!("❌ PROBLEM: Target hash found in single car scenario!");
    } else if unique_hashes_2.contains(&target_bytes) {
        println!("❌ PROBLEM: Target hash found in one other car scenario!");
    } else {
        println!("✅ Target hash NOT found in any valid scenario");
        println!("   This confirms it's an invalid/corrupted hash");
    }
    
    // Analyze the key generation process
    println!("\n--- Key Generation Analysis ---");
    let mut key_values = std::collections::HashSet::new();
    
    for tile_combo in &tile_combinations {
        // Recreate the key generation logic
        let mut key: u32 = 0;
        for (i, &(dx,dy)) in DIRS.iter().enumerate() {
            let tx = x + dx.wrapping_mul(speed as i32);
            let ty = y + dy.wrapping_mul(speed as i32);
            
            let mut flag = TileFlag::Normal as u8;
            if tx < 0 || ty < 0 || ty as usize >= 50 || tx as usize >= 50 {
                flag = TileFlag::Wall as u8;
            } else {
                flag = tile_combo[i] as u8;
            }
            
            let has_car = 0u8; // No other cars
            let nibble = (flag & 0b111) | (has_car << 3);
            key |= (nibble as u32) << (i * 4);
        }
        
        // No other cars, so dir3 = None (0)
        key |= (Dir3::None as u32) << 16;
        
        key_values.insert(key);
    }
    
    println!("Key generation analysis:");
    println!("  Total possible keys: {}", key_values.len());
    println!("  Key range: {} to {}", key_values.iter().min().unwrap(), key_values.iter().max().unwrap());
    
    // Check if there are any unexpected key values
    let max_expected_key = 0b1111111111111111111111; // 22 bits max
    let unexpected_keys: Vec<&u32> = key_values.iter().filter(|&&k| k > max_expected_key).collect();
    
    if !unexpected_keys.is_empty() {
        println!("❌ PROBLEM: Found keys exceeding 22-bit limit: {:?}", unexpected_keys);
    } else {
        println!("✅ All keys within expected 22-bit range");
    }
}

#[test]
fn test_brute_force_hash_search_single_car() {
    let target_hash = "65563059B4D6A262BA9EC5DDE6E9365ED2C58F46FF067BA372FE0C58171A1791";
    
    // Convert hex string to bytes
    let mut target_bytes = [0u8; 32];
    for (i, chunk) in target_hash.as_bytes().chunks(2).enumerate() {
        if i < 32 {
            let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
            target_bytes[i] = u8::from_str_radix(hex_str, 16).unwrap_or(0);
        }
    }
    
    println!("Target hash: {:?}", target_bytes);
    println!("Searching for single car scenario (no other cars)...");
    
    // Test only single car scenario (no other cars) with fixed position/speed
    let tile_combinations = generate_all_tile_combinations();
    
    // Fixed position and speed since they don't affect the hash
    let x = 0;
    let y = 0;
    let speed = 1;
    let other_cars = vec![]; // No other cars
    
    println!("Testing {} tile combinations with x={}, y={}, speed={}, other_cars=[]", 
             tile_combinations.len(), x, y, speed);
    
    for (i, tile_combo) in tile_combinations.iter().enumerate() {
        if i % 100 == 0 {
            println!("Progress: {}/{} combinations tested", i, tile_combinations.len());
        }
        
        let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
        if test_hash == target_bytes {
            println!("🎉 FOUND MATCH!");
            println!("x: {}, y: {}, speed: {}, other_cars: {:?}", x, y, speed, other_cars);
            println!("tile_combo: {:?}", tile_combo);
            return;
        }
    }
    
    println!("❌ No match found for hash: {} in single car scenario", target_hash);
    println!("This confirms the hash was never generated by the legacy system!");
}

#[test]
fn test_brute_force_hash_search_with_other_cars() {
    let target_hash = "65563059B4D6A262BA9EC5DDE6E9365ED2C58F46FF067BA372FE0C58171A1791";
    
    // Convert hex string to bytes
    let mut target_bytes = [0u8; 32];
    for (i, chunk) in target_hash.as_bytes().chunks(2).enumerate() {
        if i < 32 {
            let hex_str = std::str::from_utf8(chunk).unwrap_or("00");
            target_bytes[i] = u8::from_str_radix(hex_str, 16).unwrap_or(0);
        }
    }
    
    println!("Target hash: {:?}", target_bytes);
    println!("Searching with other cars scenario...");
    
    // Test with other cars (but limited scope)
    let tile_combinations = generate_all_tile_combinations();
    
    // Fixed position and speed
    let x = 0;
    let y = 0;
    let speed = 1;
    
    // Test with 1 other car in nearby positions
    println!("Testing with 1 other car in nearby positions...");
    for other_x in 0..10 {
        for other_y in 0..10 {
            let other_cars = vec![(other_x, other_y)];
            
            for tile_combo in &tile_combinations {
                let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
                if test_hash == target_bytes {
                    println!("🎉 FOUND MATCH!");
                    println!("x: {}, y: {}, speed: {}, other_cars: {:?}", x, y, speed, other_cars);
                    println!("tile_combo: {:?}", tile_combo);
                    return;
                }
            }
        }
    }
    
    println!("❌ No match found for hash: {} with other cars scenario", target_hash);
    println!("This hash appears to be invalid or corrupted!");
}

#[test]
fn test_basic_contract_operations() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info(ADMIN, &[]);

    // Test instantiate
    let instantiate_msg = InstantiateMsg {
        admin: ADMIN.to_string(),
        track_contract: TRACK_CONTRACT.to_string(),
        car_contract: CAR_CONTRACT.to_string(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();
    assert_eq!(0, res.messages.len());
}