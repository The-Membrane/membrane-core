# Transmuter

> **Organ:** Transmuter

## Purpose & Role

The Transmuter is a swap vault enabling 1:1 CDT/USDC conversion with vault token (VT) accounting. Users deposit paired assets to earn swap fees via VT appreciation. The CDP contract uses it for peg debt swaps (CDT -> USDC). The Acquisition contract uses it as the deposit destination for user acquisition deposits.

## State

| State Key | Type | Description |
|-----------|------|-------------|
| `CONFIG` | `Config` | Contract configuration |
| `VAULT_TOKEN_SUPPLY` | `Uint128` | Total VT in circulation |
| `ACQUISITION_DEPOSIT_TOTAL` | `Uint128` | Total deposits from acquisition contract (no VTs minted) |
| `TRANSMUTE_HISTORY` | `Vec<SwapRecord>` | Capped swap history |
| `VOLUME_WINDOW` | Single window | Current epoch volume tracker |
| `VOLUME_HISTORY` | `Vec` (capped) | Historical volume windows |
| `CUMULATIVE_VOLUME` | `Uint128` | All-time swap volume |
| `RATE_HISTORY` | Max 365 entries | VT conversion rate snapshots (24hr rate-limited) |
| `AFFILIATES` | `Map<Addr, Vec<AffiliateData>>` | Affiliate data, limit 10 per user |
| `TOKEN_RATE_ASSURANCE` | Pre-tx snapshot | Stores VT rate before deposit/withdrawal |
| `PENDING_REVENUE` | `Uint128` | Undistributed revenue |

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_VAULT_TOKENS_PER_STAKED_BASE_TOKEN` | 1,000,000 | Initial VT mint ratio |
| `SECONDS_PER_DAY` | 86,400 | — |
| Rate history cap | 365 | Max rate snapshots |
| `AFFILIATE_LIMIT` | 10 | Max affiliates per user |
| Conversion rate scale | 10^12 | Precision for VT rate math |
| Default `usage_fee` | 1% | Fee on swaps |
| Default `usage_fee_utilization_threshold` | 80% | **Exists in config but NEVER checked in `execute_transmute`** |
| Default `send_swap_fee` | true | Route fee portion to Revenue Distributor |
| Default `revenue_distributor_fee_percentage` | 20% | Portion of fee sent externally |
| Default `retention_multiplier` | `Decimal::zero()` | **Stored but NEVER used** |
| History slice default | 50 | Default query page size |
| History slice max | 100 | Maximum query page size |

## Key Concepts

### Vault Tokens (VT)

Depositors receive VT representing a proportional claim on the vault's CDT + USDC:

**Mint formula:**
- If `total_deposits == 0`: `VT = base_tokens * 1_000_000`
- Else: `VT = supply * base_tokens / total_deposits`

**Burn formula:**
- If `supply == 0`: `base_tokens = VT / 1_000_000`
- Else: `base_tokens = total_deposits * VT / supply`

VT rate increases as swap fees accumulate in the vault, since fees add to `total_deposits` without minting new VT.

### Deposit Alignment

Non-revenue-distributor senders are checked via `ensure_deposit_alignment`:

```
effective_cdt_target = max(config.cdt_target_ratio, deployed_paired_asset / total_deposits)
```

This ensures deposits maintain the vault's target composition. When paired assets are deployed to external venues, the effective target adjusts upward.

### Swap Restrictions

| Direction | Who Can Execute |
|-----------|----------------|
| `paired_asset -> CDT` | Anyone |
| `CDT -> paired_asset` | CDP contract + self **only** |

All other addresses attempting CDT -> paired_asset receive `CdtToPairedAssetRestricted` error.

### Usage Fee

- Default: 1% of input amount
- Applied to ALL non-allowlisted swaps unconditionally
- `usage_fee_utilization_threshold` (80%) exists in `Config` but the code **never reads it** during `execute_transmute`
- Fee distribution when `send_swap_fee == true`: 20% to Revenue Distributor, 80% stays in vault

### Rate Assurance

A self-callback mechanism that prevents VT rate manipulation:

1. Before deposit/withdrawal: save current rate to `TOKEN_RATE_ASSURANCE`
2. Execute the deposit/withdrawal operation
3. Queue `RateAssurance` self-call (executed in same tx)
4. `RateAssurance` computes `new_rate = regular_pool / vt_supply`
5. If `new_rate < pre_rate`: **entire transaction reverts**

Only callable by the contract itself (self-call auth).

### Acquisition Deposit Tracking

Deposits from the Acquisition contract are handled differently:

- Recipient is the acquisition contract address -> `ACQUISITION_DEPOSIT_TOTAL` incremented
- **No VTs are minted** for acquisition deposits
- Underlying assets are held in the vault but tracked separately
- `TransferDepositOwnership` converts acquisition deposits to VTs when users claim

## Execute Messages

### EnterVault

Deposit CDT or USDC into the vault.

1. Segregates incoming funds by type (CDT vs paired_asset)
2. Non-revenue-distributor senders: `ensure_deposit_alignment` check against effective CDT target
3. Saves pre-tx VT rate to `TOKEN_RATE_ASSURANCE`
4. **If recipient == acquisition contract**: increment `ACQUISITION_DEPOSIT_TOTAL`, NO VTs minted
5. **Otherwise**: calculate VTs via `calculate_vault_tokens`, mint via tokenfactory, increment `VAULT_TOKEN_SUPPLY`
6. Queue `RateAssurance` self-call
7. Send `AccruePool` to acquisition contract

### ExitVault

Withdraw from the vault.

**Acquisition path** (sender is acquisition contract):
- Uses raw `amount` parameter for withdrawal sizing
- Proportional withdrawal from CDT + paired_asset pools
- Decrements `ACQUISITION_DEPOSIT_TOTAL`

**Regular path**:
- Reads VT amount from `info.funds`
- `calculate_base_tokens` converts VTs to underlying value
- Proportional withdrawal from both pools
- Burns VTs via tokenfactory, decrements `VAULT_TOKEN_SUPPLY`
- `withdraw_as` option allows single-sided withdrawal (receive only CDT or only paired_asset)

Both paths: queue `RateAssurance` self-call, notify acquisition contract.

### Transmute

Swap between CDT and paired_asset at 1:1.

1. Exactly ONE asset must be sent
2. CDT -> paired_asset: restricted to CDP contract + self (error for others)
3. Fee: `usage_fee * input_amount` (no utilization check)
4. `receive_amount = input_amount - fee` (1:1 swap after fee)
5. Deployed paired asset tracking updated for allowlisted venues
6. Fee split: if `send_swap_fee`, 20% to Revenue Distributor via `CosmosMsg`, 80% stays in vault
7. Swap recorded in `TRANSMUTE_HISTORY`, volume updated

### TransferDepositOwnership

Converts acquisition deposits to regular VT deposits.

- Only callable when the user's deposit is the acquisition contract (checked by deposit ownership)
- Decrements `ACQUISITION_DEPOSIT_TOTAL` by the transfer amount
- Calculates VTs at current rate via `calculate_vault_tokens`
- Mints VTs to `new_owner` via tokenfactory
- Increments `VAULT_TOKEN_SUPPLY`

### UpdateVolumeWindow

Permissionless. Rotates the current volume tracking epoch. Volume history is stored as a capped vector.

### AddToRateHistory

Permissionless. Snapshots the current VT conversion rate. Rate-limited to once per 24 hours. Capped at 365 entries.

### SetAffiliate

Sets affiliate data for the caller. Limited to 10 affiliates per user. Data is stored but affiliate fee payout logic is dead code.

### RateAssurance

Self-call only. Compares post-operation VT rate against pre-operation snapshot. Reverts if rate decreased.

## Query Messages

| Query | Returns | Description |
|-------|---------|-------------|
| `Config` | `Config` | Full configuration |
| `VaultInfo` | `VaultInfoResponse` | Total deposits, CDT balance, paired_asset balance, VT supply |
| `TransmuteHistory` | `Vec<SwapRecord>` | Recent swap records (paginated, max 100) |
| `VolumeHistory` | Volume windows | Historical volume data (paginated, max 100) |
| `DeployedPairedAsset` | `Uint128` | Paired asset deployed to external venues |
| `EffectiveTarget` | `Decimal` | Effective CDT target ratio after deployment adjustment |
| `GetAffiliates` | `Vec<AffiliateData>` | User's affiliate data |
| `RateHistory` | Rate snapshots | VT conversion rate history (max 365) |
| `VaultTokenUnderlying` | `Uint128` | Base token value for a given VT amount |
| `AcquisitionDepositTotal` | `Uint128` | Total acquisition-tracked deposits |

## Config

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `deposit_pair` | `AssetPair` | — | CDT and paired asset denoms |
| `cdt_target_ratio` | `Decimal` | 0% | Target CDT composition ratio |
| `usage_fee` | `Decimal` | 1% | Fee on swaps (applied unconditionally) |
| `usage_fee_utilization_threshold` | `Decimal` | 80% | **Stored but never read in execute_transmute** |
| `allowlist` | `Vec<String>` | — | Addresses exempt from usage fees |
| `send_swap_fee` | `bool` | true | Whether to route fee portion to Revenue Distributor |
| `revenue_distributor_fee_percentage` | `Decimal` | 20% | Portion of fees sent to Revenue Distributor |
| `retention_multiplier` | `Decimal` | `Decimal::zero()` | **Stored but never used in contract logic** |
| `revenue_distributor` | `Option<Addr>` | — | Revenue Distributor address |
| `acquisition_contract` | `Option<Addr>` | — | Acquisition contract address |
| `cdp_contract` | `Option<Addr>` | — | CDP contract (authorized for CDT->paired swaps) |

## Cross-Contract Interactions

### Calls (Outgoing)

| Contract | Message | Purpose |
|----------|---------|---------|
| **Revenue Distributor** | BankMsg::Send (CDT) | 20% of swap fees |
| **Acquisition** | `AccruePool` | Notify on deposit/withdrawal for pool accrual |

All outgoing calls are fire-and-forget `CosmosMsg` with no reply handlers.

### Called By (Incoming)

| Contract | Message | Purpose |
|----------|---------|---------|
| **CDP (Debt)** | `Transmute` | CDT -> paired_asset peg swaps |
| **Acquisition** | `EnterVault` | Deposit user acquisition funds (no VTs minted) |
| **Acquisition** | `ExitVault` | Withdraw acquisition deposits |
| **Revenue Distributor** | `EnterVault` | Convert revenue CDT to VT |
| **Users** | `EnterVault`, `ExitVault`, `Transmute` | LP and swap operations |

## Important Invariants & Edge Cases

- **Rate assurance**: VT conversion rate is snapshotted before and checked after every deposit and withdrawal. Any rate decrease during a single operation causes the entire transaction to revert
- **Acquisition deposits have no VTs**: Deposits where the recipient is the acquisition contract increment `ACQUISITION_DEPOSIT_TOTAL` but mint zero VTs. The underlying assets sit in the vault alongside regular deposits
- **TransferDepositOwnership mints VTs**: When a user claims their acquisition deposit, VTs are minted at the current rate. This means acquisition depositors participate in rate changes between deposit and claim
- **Dead code**: `split_affiliate_fee`, `update_affiliates` exist but are `#[allow(dead_code)]`. `retention_multiplier` is stored but never read. `usage_fee_utilization_threshold` is in config but never checked during swaps
- **No sliding windows**: Volume tracking is epoch-based, rotated manually via `UpdateVolumeWindow`. There are no automatic sliding window rate limits
- **Volume and history caps**: `TRANSMUTE_HISTORY`, `VOLUME_HISTORY`, and `RATE_HISTORY` are all capped to prevent unbounded state growth
- **Single-sided withdrawal**: `withdraw_as` in `ExitVault` allows receiving only CDT or only paired_asset, but the VT burn still reflects proportional value
