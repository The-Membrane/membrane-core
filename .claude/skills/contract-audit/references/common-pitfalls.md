# Common Pitfall Patterns
## Duplicate Arrays, Unbounded Loops, Refunds, Arithmetic

---

## Duplicate Array Elements (CRITICAL)

Functions accepting `Vec<>` without dedup allow users to claim rewards multiple times.

```rust
// VULNERABLE
pub fn claim_rewards(deps: DepsMut, claim_ids: Vec<u64>) -> Result<Response> {
    let mut total = Uint128::zero();
    for claim_id in claim_ids { // No dedup! [1,1,1,1,1] claims 5x
        let reward = REWARDS.load(deps.storage, claim_id)?;
        total += reward.amount;
        REWARDS.update(deps.storage, claim_id, |r| Ok(Reward { amount: Uint128::zero(), ..r.unwrap() }))?;
    }
    transfer_rewards(info.sender, total)?;
}

// SECURE — Check for duplicates
use std::collections::HashSet;
let unique_ids: HashSet<u64> = claim_ids.iter().copied().collect();
require!(unique_ids.len() == claim_ids.len(), "Duplicate IDs");
```

**Search**: Find `Vec<` in function params → check for `HashSet`/`dedup`/`unique`.

---

## Unbounded Iterations (MAJOR/CRITICAL)

Loops over user-controlled collections can hit gas limits, permanently locking funds.

```rust
// VULNERABLE — From Lido (funds stuck forever)
pub fn withdraw(deps: DepsMut) -> Result<Response> {
    let unbonds = UNBOND_HISTORY.load(deps.storage, &info.sender)?;
    for unbond in unbonds { // Could be 10,000+ entries
        if unbond.is_ready() { process_unbond(unbond)?; }
    }
}
```

**Solutions**:
- Hard caps: `MAX_POSITIONS_PER_USER = 100`
- Pagination: `start_after + limit` parameters
- Cleanup: Remove completed entries

**Recommended caps**: Positions/user: 100, Markets: 50, Validators: 150, History: 1000, Query results: 30.

---

## Double Refunds (CRITICAL)

Excess calculated twice → pool drained.

```rust
// VULNERABLE — From Astroport
pub fn swap_exact_amount_out(offer_asset: Asset, target: Uint128) -> Result<Response> {
    let required = calculate_required_offer(target)?;
    let excess = offer_asset.amount - required;
    if excess > Uint128::zero() { msgs.push(refund_msg(excess)); }
    // ... more code ...
    let excess = offer_asset.amount - required; // SECOND calculation!
    if excess > Uint128::zero() { msgs.push(refund_msg(excess)); } // Double refund!
}
```

**Search**: Look for duplicate variable names (`excess`, `refund`, `remaining`).

---

## Wrong Denomination (MAJOR)

```rust
// VULNERABLE — From Anchor
pub fn submit_bid(deps: DepsMut, info: MessageInfo) -> Result<Response> {
    let stable = info.funds.iter()
        .find(|c| c.denom == config.stable_denom)
        .map(|c| c.amount).unwrap_or_default();
    // Other denoms silently accepted and LOCKED FOREVER
}
```

**Rule**: Validate ALL received coin denominations. Reject or refund wrong denoms.

---

## Precision Loss

```rust
// VULNERABLE — division before multiplication
let rate = amount / total;     // Truncated to 0!
rate * TOTAL_SHARES            // = 0

// CORRECT — multiply first
amount.checked_mul(TOTAL_SHARES)?.checked_div(total)?
```

**Rule**: Always multiply before dividing with integers.

---

## Checks-Effects-Interactions Violation

```rust
// VULNERABLE — state after external call
fn withdraw(amount: Uint128) -> Result<Response> {
    let balance = BALANCES.load(user)?;
    transfer_tokens(user, amount)?;          // External call FIRST
    BALANCES.save(user, &(balance - amount))?; // State update AFTER → reentrancy
}
```

**Rule**: Load → State Update → External Call. Check every `WasmMsg::Execute` and `BankMsg::Send`.
