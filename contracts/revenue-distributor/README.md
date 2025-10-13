# Revenue Distributor Contract

## Overview

The Revenue Distributor contract is a specialized CosmosWasm contract designed to handle the distribution of revenue from the CDP (Collateralized Debt Position) contract. It separates revenue distribution logic from the main CDP contract, providing better modularity and error handling.

## Purpose

The Revenue Distributor serves as an intermediary between the CDP contract and revenue recipients, managing:

1. **Affiliate Fee Distribution**: Distributes affiliate fees to affiliate addresses via bank transfers
2. **Revenue Destination Distribution**: Distributes remaining revenue to configured destinations (typically staking contracts) via `DepositFee` calls
3. **Error Handling**: Uses `reply_on_error` to ensure failed distributions don't cancel the entire transaction

## Architecture

### Key Components

- **RevenuePromise**: Struct containing recipient address and asset amount for distribution
- **Config**: Contract configuration including owner and revenue destinations
- **RevenueDestination**: Configuration for automatic revenue distribution to staking contracts

### Integration with CDP Contract

The CDP contract's `credit_burn_rev_msg` function now:

1. Calculates affiliate fees based on position affiliates
2. Creates `RevenuePromise` objects for each affiliate
3. Sends all revenue to the Revenue Distributor via `SetPromises`
4. The Revenue Distributor handles both affiliate distribution and revenue destination distribution

## Contract Functions

### Execute Messages

#### `SetPromises`
- **Purpose**: Set revenue promises for distribution
- **Validation**: Ensures total promised amount ≤ sent amount
- **Access**: Anyone who sends the correct amount of funds
- **Behavior**: Any excess amount will be distributed to revenue destinations

#### `DistributePromises`
- **Purpose**: Distribute all current promises and clear them
- **Error Handling**: Uses `reply_on_error` for failed distributions
- **Behavior**: Continues with other distributions if some fail

#### `UpdateConfig`
- **Purpose**: Update contract configuration (admin only)
- **Access**: Only contract owner
- **Fields**: Revenue destinations

### Query Messages

#### `Config`
- Returns current contract configuration

#### `Promises`
- Returns current revenue promises awaiting distribution

#### `ContractInfo`
- Returns basic contract information

## Revenue Distribution Flow

1. **CDP Contract** calculates revenue and affiliate fees
2. **CDP Contract** sends revenue to Revenue Distributor via `SetPromises`
3. **Revenue Distributor** validates promises against sent amount
4. **Revenue Distributor** distributes promises via `DistributePromises`:
   - Affiliate fees → Bank transfers to affiliate addresses
   - Remaining revenue → `DepositFee` calls to revenue destinations
5. **Failed distributions** are tracked but don't cancel the transaction

## Error Handling

- **Invalid Amount**: Rejected if promised amount > sent amount
- **No Promises**: Error if trying to distribute without promises set
- **Failed Distributions**: Tracked separately, don't cancel entire transaction

## Security Considerations

- **Owner-Only Config Updates**: Only contract owner can modify configuration
- **Amount Validation**: Strict validation of promised vs sent amounts
- **Error Isolation**: Failed distributions don't affect successful ones

## Configuration

The contract requires:

- **Owner Address**: Admin address for configuration updates
- **Revenue Destinations**: List of addresses (typically staking contracts) for automatic revenue distribution
- **Distribution Ratios**: Percentages for each revenue destination

## Usage Example

```rust
// CDP contract sends revenue to distributor
let promises = vec![
    RevenuePromise {
        address: "affiliate1_addr".to_string(),
        asset: Asset { amount: 1000, info: AssetInfo::NativeToken { denom: "ucdt" } }
    }
];

let msg = ExecuteMsg::SetPromises { promises };
// Send with funds containing the revenue amount
```

## Benefits

1. **Modularity**: Separates revenue distribution from CDP logic
2. **Error Resilience**: Failed distributions don't cancel entire transaction
3. **Flexibility**: Supports both affiliate fees and revenue destinations
4. **Maintainability**: Easier to update distribution logic independently
5. **Gas Efficiency**: Optimized for batch distribution operations

## Future Enhancements

- **Retry Mechanism**: Automatic retry for failed distributions
- **Distribution Scheduling**: Time-based distribution triggers
- **Multi-Asset Support**: Support for multiple asset types in single distribution
- **Analytics**: Distribution tracking and reporting features
