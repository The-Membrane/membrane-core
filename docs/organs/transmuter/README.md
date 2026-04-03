# Transmuter Organ

The Transmuter is Membrane's **CDT peg stabilization mechanism** — a swap vault that allows 1:1 conversion between CDT and a paired asset (USDC). It functions as a PSM (Peg Stability Module) with vault token accounting, deposit alignment enforcement, and fee distribution.

## Contracts in this Organ

| Contract | Purpose |
|----------|---------|
| [Transmuter](transmuter.md) | CDT/USDC swap vault with vault tokens, rate assurance, acquisition integration |

## Why the Transmuter Exists

CDT needs a reliable way to maintain its peg to $1. The Transmuter provides:

1. **Peg floor**: The CDP contract can swap CDT -> USDC at 1:1 (minus usage fee)
2. **Peg ceiling**: Users can swap USDC -> CDT at 1:1
3. **LP yield**: Depositors earn swap fees via increasing vault token rates
4. **TVL anchor**: The Transmuter's USDC deposits back the Acquisition system's deposit capacity

## Architecture

```
     Users                          CDP Contract
       |                                |
  Deposit USDC/CDT              CDT -> USDC swaps
  (receive VTs)                 (ONLY direction for CDP)
       |                                |
       v                                |
  +---------------------------------------------+
  |            TRANSMUTER                         |
  |                                               |
  |  CDT Pool <----------> USDC Pool             |
  |                                               |
  |  Swap Direction:                              |
  |    paired_asset -> CDT: Anyone                |
  |    CDT -> paired_asset: CDP contract ONLY     |
  |                                               |
  |  Usage Fee: 1% on ALL non-allowlisted swaps   |
  |  (utilization threshold exists in config      |
  |   but is NEVER checked in execute_transmute)  |
  |                                               |
  |  Fee Split: 20% Revenue Dist, 80% vault       |
  +------------------+---------------------------+
                     |
             +-------+--------+
             |                |
        Revenue           Acquisition
        Dist.             (deposit tracking,
        (20% fees)        AccruePool notifications)
```

## Key User Journey

### LP Depositing

1. Deposit USDC or CDT via `EnterVault`
2. Deposit alignment enforced: effective CDT target = max(config.cdt_target_ratio, deployed_paired / total_deposits)
3. Receive Vault Tokens (VT) at current conversion rate (default 1:1,000,000 if first deposit)
4. VT rate increases as swap fees accumulate in the vault
5. Withdraw via `ExitVault` — burn VT, receive proportional USDC + CDT (or single-sided via `withdraw_as`)

### Swapping (Transmuting)

1. Send exactly ONE asset with `Transmute`
2. CDT -> paired_asset is **restricted to CDP contract + self only** (all other users receive `CdtToPairedAssetRestricted` error)
3. USDC -> CDT is open to anyone
4. Usage fee (default 1%) applied to input amount unconditionally for non-allowlisted addresses
5. Swap is 1:1 after fee: `receive_amount = input - fee`
6. Fee distribution: if `send_swap_fee` is true, 20% to Revenue Distributor, 80% stays in vault

## Cross-Organ Dependencies

| Dependency | How It's Used |
|-----------|---------------|
| **CDP (Debt)** | Only contract authorized for CDT -> USDC swaps. Also acts as deployment venue |
| **Revenue Distributor** | Receives 20% of swap fees (configurable) |
| **Acquisition** | Deposits tracked separately via `ACQUISITION_DEPOSIT_TOTAL`. AccruePool notifications sent on enter/exit |

## Critical Mechanics

### Swap Direction Restriction

CDT -> paired_asset swaps are **restricted to the CDP contract only**. Regular users can only swap paired_asset -> CDT. This is a hard restriction in the code, not a configurable parameter.

### No Sliding Windows

There are no sliding windows for rate limiting. Volume tracking uses manual epoch-based rotation via the `UpdateVolumeWindow` message. Volume history is stored as a capped vector.

### Affiliate System (Dead Code)

Affiliate data is stored (up to 10 per user via `AFFILIATES` map), but the fee splitting and payout functions (`split_affiliate_fee`, `update_affiliates`) are marked `#[allow(dead_code)]` and are never called during transmutation.

### Retention Multiplier (Unused)

`retention_multiplier` exists in `Config` storage but is never read or applied anywhere in `contract.rs`.

### No Reply Handlers

All cross-contract calls (Revenue Distributor fees, Acquisition notifications) are fire-and-forget `CosmosMsg`. There are no `reply` handlers in the Transmuter contract.
