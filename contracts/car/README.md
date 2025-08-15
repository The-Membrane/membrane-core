# Car NFT Contract

This contract implements a CW721-compatible NFT system for racing cars with Q-learning state storage.

## Features

- **Car NFT Management**: Mint, transfer, and manage car NFTs
- **Q-Table Storage**: Store Q-learning state-action values for each car
- **Metadata Support**: Rich metadata for car attributes and traits
- **Query Interface**: Comprehensive querying for car info and Q-values

## Messages

### Execute Messages

- `Mint`: Create a new car NFT with owner and optional metadata
- `UpdateCarMetadata`: Update car metadata (owner only)
- `Transfer`: Transfer car ownership

### Query Messages

- `GetCarInfo`: Get car information including owner and metadata
- `GetQ`: Query Q-table values (single state or all states)
- `OwnerOf`: Get car owner
- `NftInfo`: Get NFT information
- `AllTokens`: List all car tokens

## State Structure

- `CAR_INFO`: Maps car_id to CarInfo (owner, metadata, created_at)
- `Q_TABLE`: Maps (car_id, state_hash) to [i32; 5] action values
- `ALL_CARS`: Tracks all car IDs for enumeration

## Usage

1. **Instantiate** with admin address
2. **Mint** cars with owner and metadata
3. **Query** car information and Q-values
4. **Transfer** car ownership as needed

## Integration

This contract integrates with:
- **Trainer Contract**: For Q-table updates
- **Race Engine**: For Q-value queries during simulation
- **Tournament Contract**: For car eligibility checks 