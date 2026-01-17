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

fn main() {
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
    
    // Test all possible combinations
    let tile_combinations = generate_all_tile_combinations();
    
    // Test different positions
    for x in 0..50 {
        for y in 0..50 {
            // Test different speeds
            for speed in 1..=5 {
                // Test with no other cars
                let other_cars = vec![];
                for tile_combo in &tile_combinations {
                    let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
                    if test_hash == target_bytes {
                        println!("FOUND MATCH!");
                        println!("x: {}, y: {}, speed: {}, other_cars: {:?}", x, y, speed, other_cars);
                        println!("tile_combo: {:?}", tile_combo);
                        return;
                    }
                }
                
                // Test with 1 other car
                for other_x in 0..50 {
                    for other_y in 0..50 {
                        let other_cars = vec![(other_x, other_y)];
                        for tile_combo in &tile_combinations {
                            let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
                            if test_hash == target_bytes {
                                println!("FOUND MATCH!");
                                println!("x: {}, y: {}, speed: {}, other_cars: {:?}", x, y, speed, other_cars);
                                println!("tile_combo: {:?}", tile_combo);
                                return;
                            }
                        }
                    }
                }
                
                // Test with 2 other cars
                for other_x1 in 0..50 {
                    for other_y1 in 0..50 {
                        for other_x2 in 0..50 {
                            for other_y2 in 0..50 {
                                if (other_x1, other_y1) != (other_x2, other_y2) {
                                    let other_cars = vec![(other_x1, other_y1), (other_x2, other_y2)];
                                    for tile_combo in &tile_combinations {
                                        let test_hash = generate_legacy_state_hash_for_migration(x, y, speed, &other_cars, *tile_combo);
                                        if test_hash == target_bytes {
                                            println!("FOUND MATCH!");
                                            println!("x: {}, y: {}, speed: {}, other_cars: {:?}", x, y, speed, other_cars);
                                            println!("tile_combo: {:?}", tile_combo);
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    println!("No match found for hash: {}", target_hash);
}
