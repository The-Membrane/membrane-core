# Discount Vault

> **Category:** Infrastructure

## Purpose

The Discount Vault allows users to deposit accepted LP tokens (e.g., CDT-OSMO LP shares from Osmosis GAMM pools) to earn protocol discounts via the System Discounts contract. Deposited LPs are tracked per user with timestamps, enabling time-based discount accrual.

**Source:** `contracts/discount_vault/src/contracts.rs`, `contracts/discount_vault/src/state.rs`

## Execute Messages

| Message | Purpose | Auth |
|---------|---------|------|
| `Deposit {}` | Deposit accepted LP tokens (sent as funds) | Any user |
| `Withdraw { withdrawal_assets }` | Withdraw deposited LP tokens | Depositor only |
| `ChangeOwner { owner }` | Transfer contract ownership | Owner |
| `EditAcceptedLPs { pool_ids, remove }` | Add or remove accepted LP pool IDs | Owner |
| `ToggleDeposits { enable }` | Enable/disable new deposits | Owner |

## LP Validation

When LP pool IDs are added via `EditAcceptedLPs`, each pool is validated:

1. Queries Osmosis Proxy for pool state (`get_pool_state_response`)
2. Extracts share token denom (`gamm/pool/{id}`)
3. Validates that the LP pool contains the debt token (CDT) by querying the CDP contract's `GetBasket`
4. If the pool does not contain CDT, the LP is rejected

This ensures only LPs that include CDT can be deposited, since discount credit should correlate with CDT liquidity provision.

```rust
fn create_and_validate_LP_object(querier, pool_id, positions_contract, osmosis_proxy) -> LPPoolInfo {
    let res = get_pool_state_response(querier, osmosis_proxy, pool_id);
    let share_token = AssetInfo::NativeToken { denom: res.shares.denom };
    let basket: Basket = querier.query_wasm_smart(positions_contract, &CDPQueryMsg::GetBasket {});
    // Verify LP contains debt token
    assert!(res.assets.iter().any(|a| a.denom == basket.credit_asset.info.to_string()));
    LPPoolInfo { share_token, pool_id }
}
```

## Deposit Flow

1. User sends LP tokens as message funds
2. Contract validates all sent tokens against `config.accepted_LPs`
3. If user exists: pushes new `VaultedLP` entries to their deposit list
4. If new user: creates `VaultUser` with the deposited LPs

Each deposit records:
```rust
pub struct VaultedLP {
    pub gamm: AssetInfo,     // LP token denom
    pub amount: Uint128,     // Amount deposited
    pub deposit_time: u64,   // Block timestamp (used for time-based discount)
}
```

The `deposit_time` is critical for the System Discounts time curve -- longer deposits earn higher discounts.

## Withdrawal Flow

1. User specifies `withdrawal_assets` (list of assets to withdraw)
2. Contract verifies user owns the requested assets
3. Deducts amounts from user's `vaulted_lps`
4. Sends tokens back via `multi_native_withdrawal_msg`

Withdrawal removes deposits in FIFO order from the user's deposit list.

## State

| Key | Type | Purpose |
|-----|------|---------|
| `CONFIG` | `Item<Config>` | Contract configuration |
| `USERS` | `Map<Addr, VaultUser>` | Per-user deposit tracking |
| `OWNERSHIP_TRANSFER` | `Item<Addr>` | Pending ownership transfer |

### Config

```rust
pub struct Config {
    pub owner: Addr,
    pub positions_contract: Addr,     // CDP contract for basket queries
    pub osmosis_proxy: Addr,          // For pool state queries
    pub accepted_LPs: Vec<LPPoolInfo>,
    pub deposits_enabled: bool,
}
```

### VaultUser

```rust
pub struct VaultUser {
    pub user: Addr,
    pub vaulted_lps: Vec<VaultedLP>,
}
```

## Integration with System Discounts

The System Discounts contract queries the Discount Vault to calculate a user's discount credit:

1. Queries `VaultUser` for the user's deposited LPs
2. For each LP, calculates its CDT-denominated value using pool composition
3. Applies the time curve discount based on `deposit_time`:
   ```
   days_since_deposit = (current_time - deposit_time) / 86400
   discount = calculate_time_curve_discount(days_since_deposit, ...)
   ```
4. Longer deposits earn progressively higher discounts up to `max_discount`

The discount reduces CDP borrowing fees for the user, creating an incentive to provide CDT liquidity via LP tokens.

## Relationship to Other Contracts

- **System Discounts:** Queries vault for user deposits and timestamps to calculate discounts
- **CDP / Positions:** Provides `GetBasket` for LP validation
- **Osmosis Proxy:** Provides pool state for LP validation and value calculation
