# Neutron Proxy

> **Category:** Infrastructure

## Purpose

The Neutron Proxy handles Neutron-specific operations: token factory (create/mint/burn), Astroport and Duality DEX swaps, multi-hop routing, and transmutation. It is the Neutron equivalent of the Osmosis Proxy, adapted for the Neutron chain's specific DEX infrastructure.

**Source:** `contracts/neutron-proxy/src/contract.rs`, `contracts/neutron-proxy/src/astroport_helpers.rs`

## Execute Messages

| Message | Purpose | Auth |
|---------|---------|------|
| `CreateDenom { subdenom, max_supply }` | Creates a token factory denom on Neutron | Owner |
| `ChangeAdmin { denom, new_admin_address }` | Transfers denom admin | Owner |
| `MintTokens { denom, amount, mint_to_address }` | Mints tokens with max supply check | Owner or authorized |
| `BurnTokens { denom, amount, burn_from_address }` | Burns tokens | Owner or authorized |
| `EditTokenMaxSupply { denom, max_supply }` | Updates max supply cap | Owner |
| `ExecuteSwaps { token_out, max_slippage }` | Executes swaps via Astroport/Duality with multi-hop routing | Owner or authorized |
| `TransmuteTokens {}` | Transmutes tokens through configured pairs + optional vesting | Authorized |
| `UpdateConfig { ... }` | Updates owners, swap routes, transmuter, vaults, Astroport factory/router | Owner |
| `UpdateSwapRoute { route }` | Updates a single swap route | Owner |
| `UpdateSwapRoutes { routes }` | Batch update swap routes | Owner |
| `EditOwner { owner, non_token_contract_auth, remove }` | Edits ownership list | Owner |

## Token Factory Operations

Identical pattern to Osmosis Proxy:

1. `CreateDenom` creates `factory/{contract_addr}/{subdenom}` via Neutron's token factory module (`NeutronMsg::CreateDenom`)
2. `MintTokens` enforces max supply, creates `NeutronMsg::MintTokens`
3. `BurnTokens` creates `NeutronMsg::BurnTokens`

The Neutron Proxy handles MBRN minting for the Acquisition contract (`MintTokens { mint_to_address: contract_or_user }`) and CDT minting for CDP operations.

## DEX Swap Execution

### Astroport Integration

`ExecuteSwaps` routes swaps through Astroport, with two paths:

**Direct pair swap** (`astroport_helpers.rs`):
```rust
PairExecuteMsg::Swap {
    offer_asset,
    max_spread: Some(max_slippage),
    to: None,
    ..
}
```

**Multi-hop router** (when configured):
```rust
RouterExecuteMsg::ExecuteSwapOperations {
    operations: [...],
    minimum_receive: Some(min_out),
    to: None,
    max_spread: Some(max_slippage),
}
```

### Dynamic Routing

When `enable_dynamic_routing` is enabled, the proxy can route through the Astroport router for multi-hop swaps. Swap routes are stored per input denom and can reference specific Astroport pair contracts or router operations.

### Duality Integration

The proxy also supports Duality DEX swaps for assets that have Duality liquidity pools on Neutron.

## Transmutation

`TransmuteTokens` has enhanced logic compared to the Osmosis variant:

1. Checks configured `transmutation_pairs` for matching input denom
2. If a `transmuter_contract` is configured, uses the Transmuter's `Transmute` endpoint
3. If `vaults` are configured, checks Mars Vault Token `ExitVault` for vault-based transmutation
4. Supports vesting integration: when `vesting_contract` and `vesting_period` are configured, transmutation proceeds are vested rather than sent immediately

### Vesting Threshold

The proxy supports `transmute_supply_thresholds`: when a transmutation would bring supply of a denom above a threshold, the excess is routed through the vesting contract via `AddVestedTransmutation`. This prevents sudden supply shocks from large transmutations.

## Config Structure

| Field | Purpose |
|-------|---------|
| `owners` | List of authorized owner addresses |
| `debt_auction` | Auction contract for bad debt MBRN sales |
| `transmutation_pairs` | Configured 1:1 swap pairs (denom -> denom) |
| `transmuter_contract` | Address of the Transmuter contract for CDT-USDC swaps |
| `vaults` | Vault addresses for vault-token-based transmutations |
| `astroport_factory` | Astroport factory contract for pool queries |
| `astroport_router` | Astroport router contract for multi-hop swaps |
| `enable_dynamic_routing` | Toggle for router-based multi-hop |
| `transmute_supply_thresholds` | Per-denom thresholds for vested transmutation |
| `vesting_contract` | Vesting contract for threshold-based transmutation |
| `vesting_period` | Duration for vested transmutations |
| `non_token_contract_auth` | Contracts authorized for non-token operations |

## Relationship to Other Contracts

- **Acquisition:** Calls `MintTokens` to create MBRN for emission windows and `BurnTokens` for clawback
- **CDP / Debt:** Calls `MintTokens` for CDT creation on borrow
- **Revenue Distributor:** Routes non-CDT revenue through swaps
- **Transmuter:** Uses `TransmuteTokens` for CDT-USDC conversions
- **LTV Disco:** Receives MBRN from `MintTokens` during Acquisition-to-Disco flow
- **Auction:** Handles MBRN burn after bad debt auctions
