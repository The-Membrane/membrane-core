# Liquidation Vulnerability Patterns
## From 61 Real DeFi Audit Reports

Liquidation bugs account for **40% of all critical findings**. Focus here first for any protocol with forced position closure.

---

## Pattern 1: Incomplete Asset Preservation

The liquidation function closes positions but doesn't handle ALL value-bearing fields.

**What to verify**: Find the position struct. List ALL fields representing value. Check if the liquidation function handles EACH one.

```rust
// VULNERABLE — From Mars v2
fn liquidate(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    let seized_amount = calculate_seizure(position.collateral)?;
    transfer_to_liquidator(seized_amount)?;
    POSITIONS.remove(deps.storage, position_id);
    // MISSING: position.staking_rewards, position.pending_fees
}

// SECURE
fn liquidate_complete(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    // Step 1: Claim ALL pending value BEFORE closing
    let rewards = claim_staking_rewards(&position)?;
    let fees = calculate_pending_fees(&position)?;
    // Step 2: Liquidation math
    let seized = calculate_seizure(position.collateral)?;
    let remaining = position.collateral - seized;
    // Step 3: Transfer EVERYTHING
    transfer_to_user(position.owner, remaining, rewards, fees)?;
    // Step 4: Bad debt if underwater
    if position.debt > position.collateral {
        BAD_DEBT.update(deps.storage, |total| Ok(total + position.debt - position.collateral))?;
    }
    POSITIONS.remove(deps.storage, position_id);
    Ok(Response::new())
}
```

**Severity**: Any unhandled value field = **CRITICAL**

---

## Pattern 2: Liquidation Bypass via External Calls

If liquidation calls the user's contract, the user can block their own liquidation by deploying a contract that always reverts.

```rust
// VULNERABLE — notifying user's contract
fn liquidate(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    // If user's contract reverts → entire liquidation fails
    let notify_msg = WasmMsg::Execute {
        contract_addr: position.owner.to_string(),
        msg: to_binary(&NotifyLiquidation {})?,
        funds: vec![],
    };
    Ok(Response::new().add_message(notify_msg))
}
```

**Key question**: Does liquidation make ANY external calls that could fail?

---

## Pattern 3: Zero/Dust Amount Reverts

```rust
// VULNERABLE — From Mars Perps
fn liquidate(amount: Uint128) -> Result<Response> {
    let liquidation_amount = calculate_liquidation(amount)?;
    // If amount rounds to zero, this fails → position never liquidated
    require!(liquidation_amount > Uint128::zero(), "Invalid amount");
    execute_liquidation(liquidation_amount)?;
}
```

**Key question**: What's the smallest liquidatable position? Does it round to zero?

---

## Pattern 4: Bad Debt Not Tracked

```rust
// VULNERABLE — From Margined Protocol
fn liquidate_position(position_id: u64) -> Result<Response> {
    let collateral_value = get_collateral_value(&position)?;
    let debt_value = position.debt;
    let bad_debt = if debt_value > collateral_value {
        debt_value - collateral_value
    } else { Uint128::zero() };
    // CRITICAL: bad_debt calculated but NEVER stored!
    close_position(position_id)?;
    Ok(Response::new())
}
```

**Verify**: Is there a BAD_DEBT storage variable? Is it updated on EVERY undercollateralized liquidation?

---

## Pattern 5: Reentrancy (CEI Violation)

```rust
// VULNERABLE — state update AFTER external call
fn liquidate(position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    transfer_collateral(position.collateral)?; // External call!
    POSITIONS.remove(deps.storage, position_id); // Too late!
}
```

**Rule**: Load → State Update → External Call. Never the reverse.
