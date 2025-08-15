# Race Engine Contract

A CosmWasm smart contract for simulating AI-controlled car races using Q-learning.

## Overview

The Race Engine contract executes deterministic race simulations between AI-controlled cars. Each car's behavior is driven by Q-tables stored in the Car NFT contract, and races are simulated tick-by-tick with collision detection and tile effects.

## Features

- **Deterministic Simulation**: All races are purely deterministic based on Q-tables and track state
- **Q-Learning Integration**: Cars make decisions based on their Q-tables and current sensor data
- **Collision Detection**: Cars cannot occupy the same position simultaneously
- **Tile Effects**: Different tile types affect car movement (Slow, Boost, Stick, Wall, Finish)
- **Trainer Integration**: Sends race results to Trainer contract for Q-table updates
- **Result Storage**: Maintains a ring buffer of recent race results

## Architecture

### Core Components

1. **Race Simulation**: Main simulation loop that processes car movements tick-by-tick
2. **Q-Table Loading**: Fetches Q-tables from Car NFT contract for each participating car
3. **Track Management**: Loads track layouts from Track Manager contract
4. **Result Processing**: Determines winners and sends results to Trainer contract

### State Management

- `RECENT_RACES`: Ring buffer storing last 50 race results
- `CONFIG`: Contract configuration including addresses of other contracts

### External Dependencies

- **Car NFT Contract**: Stores car Q-tables and metadata
- **Track Manager Contract**: Provides track layouts and tile data
- **Trainer Contract**: Receives race results and Q-table updates

## Usage

### Instantiation

```rust
let msg = InstantiateMsg {
    car_nft_contract: "car_nft_contract_address".to_string(),
    trainer_contract: "trainer_contract_address".to_string(),
    track_manager_contract: "track_manager_contract_address".to_string(),
};
```

### Simulating a Race

```rust
let params = SimulateRaceParams {
    track_id: "track_1".to_string(),
    car_ids: vec!["car_1".to_string(), "car_2".to_string()],
    max_ticks: Some(100),
};
```

### Querying Results

```rust
// Get specific race result
let result = QueryMsg::GetRaceResult { race_id: "race_123".to_string() };

// List recent races
let races = QueryMsg::ListRecentRaces { limit: Some(10) };
```

## Simulation Logic

### Car Movement

1. **Sensor Data**: Each car observes the 3x3 grid around its position
2. **Q-Table Lookup**: Uses sensor data to look up Q-values for available actions
3. **Action Selection**: Chooses the action with highest Q-value
4. **Movement Calculation**: Applies tile effects and calculates new position
5. **Collision Resolution**: If multiple cars target same position, none move

### Tile Effects

- **Normal**: No special effect
- **Slow**: Always move only 1 tile forward
- **Boost**: Move 3 tiles forward if chosen
- **Stick**: Move allowed, but skips the next turn
- **Wall**: Block movement completely
- **Finish**: Target for winning condition

### Termination Conditions

- Max ticks reached (default: 100)
- All cars have finished
- No cars can make progress

## Integration Points

### Trainer Contract Messages

The contract sends two types of messages to the Trainer contract:

1. **BatchUpdateQValues**: Q-table updates based on race performance
2. **RecordTrackResult**: Win/loss records per car per track

### Track Manager Integration

Loads track layouts including:
- 2D grid of tiles
- Tile types and effects
- Distance from finish for each tile

### Car NFT Integration

Fetches Q-tables for each participating car, including:
- State-action value mappings
- Car metadata and ownership

## Error Handling

The contract includes comprehensive error handling for:
- Invalid race parameters
- Missing cars or tracks
- Q-table loading failures
- Simulation failures

## Future Enhancements

- Tournament support with brackets
- Fuel/energy system for efficiency incentives
- Custom max_ticks per race
- Enhanced Q-value update algorithms
- Betting and game mode support 