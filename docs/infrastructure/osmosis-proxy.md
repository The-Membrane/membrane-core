# Osmosis Proxy

> **Category:** Infrastructure

## Purpose

The Osmosis Proxy is the bridge between Membrane's CosmWasm contracts and Osmosis SDK modules. It owns the CDT and MBRN token factory denoms on Osmosis and handles all chain-specific operations: minting, burning, DEX swaps through GAMM pools, gauge creation, and supply management.

**Source:** `contracts/osmosis-proxy/src/contract.rs`

## Execute Messages

| Message | Purpose | Auth |
|---------|---------|------|
| `CreateDenom { subdenom, max_supply }` | Creates a new token factory denom. Proxy becomes admin | Owner |
| `ChangeAdmin { denom, new_admin_address }` | Transfers denom admin to a new address | Owner |
| `MintTokens { denom, amount, mint_to_address }` | Mints tokens. Checks max supply and `restrict_mbrn_mints` | Owner or authorized contract |
| `BurnTokens { denom, amount, burn_from_address }` | Burns tokens from a specified address | Owner or authorized contract |
| `EditTokenMaxSupply { denom, max_supply }` | Updates the maximum supply cap for a denom | Owner |
| `ExecuteSwaps { token_out, max_slippage }` | Executes swaps via Osmosis GAMM pools. Sends attached funds through configured routes | Owner or authorized contract |
| `TransmuteTokens {}` | Transmutes tokens through configured transmutation pairs | Authorized |
| `AddSwapRoutesFromOracleInfo { assets }` | Auto-configures swap routes by querying Oracle for pool info | Owner |
| `CreateOsmosisGauge { gauge_msg }` | Creates an Osmosis incentive gauge | Owner |
| `EditOwner { owner, stability_pool_ratio, non_token_contract_auth }` | Edits ownership and authorization settings | Owner |
| `UpdateConfig { ... }` | Updates routes, transmutation pairs, and other configuration | Owner |

## Token Factory Operations

### Denom Ownership

The Osmosis Proxy owns all denoms it creates. When `CreateDenom` is called:
1. A new `factory/{contract_addr}/{subdenom}` denom is created via Osmosis MsgCreateDenom
2. The proxy stores the denom with its `max_supply` in config
3. All subsequent mint/burn operations go through the proxy

### Mint Guard: `restrict_mbrn_mints`

```rust
if denom.to_lowercase().contains("mbrn") && config.restrict_mbrn_mints.unwrap_or(false) {
    // Reject mint unless from authorized source
}
```

When enabled (default: `false`), this toggle prevents MBRN minting from non-authorized sources. Used as a circuit breaker if MBRN emissions need to be paused.

### Max Supply Enforcement

Every `MintTokens` call checks:
```
current_supply + amount <= max_supply
```

If exceeded, the mint is rejected. `EditTokenMaxSupply` can adjust the cap.

## DEX Swap Execution

`ExecuteSwaps` takes attached funds and routes them through Osmosis GAMM pools:

1. Looks up configured swap routes for each input denom
2. Constructs Osmosis `MsgSwapExactAmountIn` messages with `max_slippage` protection
3. Output is sent to the caller's address

Routes can be configured manually or auto-populated from Oracle pool info via `AddSwapRoutesFromOracleInfo`.

## Transmutation

`TransmuteTokens` swaps tokens through pre-configured transmutation pairs (1:1 swaps, typically CDT variants across chains or CDT-USDC via the Transmuter mechanism). Uses the Mars Vault Token `ExitVault` pattern for vault-based transmutations.

## Config Structure

Key config fields:

| Field | Purpose |
|-------|---------|
| `owners` | List of authorized addresses |
| `debt_auction` | Auction contract address for bad debt resolution |
| `positions_contract` | CDP positions contract |
| `liquidity_contract` | Liquidity check contract |
| `oracle_contract` | Oracle for price/route info |
| `transmutation_pairs` | Pre-configured 1:1 swap pairs |
| `restrict_mbrn_mints` | Circuit breaker for MBRN minting |
| `non_token_contract_auth` | Contracts authorized for non-token operations (swaps) |

## Relationship to Other Contracts

- **CDP / Debt:** Calls `MintTokens` to create CDT when users borrow
- **Revenue Distributor:** Uses swap routes to convert non-CDT revenue
- **Acquisition:** Not directly used on Osmosis (Neutron deployment uses Neutron Proxy instead)
- **Staking:** May call to execute governance-related token operations
- **Auction:** Uses `BurnTokens` after MBRN auctions to reduce supply
